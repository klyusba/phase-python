//! Python bindings for the [phase.rs](https://github.com/phase-rs/phase) rules engine.
//!
//! Load a card database, start a game, list legal actions, and apply one:
//!
//! ```python
//! from phase import Engine
//!
//! engine = Engine.from_path("card-data.json")
//! game = engine.new_game(
//!     player=["Forest"] * 60,
//!     opponent=["Forest"] * 60,
//!     seed=42,
//!     first_player=0,
//! )
//! actions = game.actions()
//! result = game.apply(0, actions[0])
//!
//! saved = game.state()
//! resumed = engine.load_game(saved)
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use engine::ai_support::legal_actions;
use engine::database::mtgjson::AtomicCard;
use engine::database::synthesis::{
    build_oracle_face as engine_build_oracle_face,
    build_oracle_face_multi as engine_build_oracle_face_multi,
};
use engine::database::CardDatabase;
use engine::game::{
    apply, load_and_hydrate_decks, rehydrate_game_from_card_db, resolve_deck_list, start_game,
    start_game_with_starting_player, validate_name_deck_for_format_full, DeckList, PlayerDeckList,
};
use engine::types::actions::{GameAction as EngineAction, GameActionKind};
use engine::types::format::{FormatConfig, GameFormat};
use engine::types::game_state::{
    ActionResult as EngineActionResult, GameState, PersistedGameState, PersistedRestoreFinalization,
};
use engine::types::identifiers::ObjectId;
use engine::types::match_config::MatchConfig;
use engine::types::player::PlayerId;
use pyo3::exceptions::{PyAttributeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict};
use serde::de::DeserializeOwned;
use serde::Serialize;

fn py_err(message: impl ToString) -> PyErr {
    PyValueError::new_err(message.to_string())
}

fn to_py<'py, T: Serialize>(py: Python<'py>, value: &T) -> PyResult<Bound<'py, PyAny>> {
    let json = serde_json::to_string(value).map_err(py_err)?;
    py.import("json")?.call_method1("loads", (json,))
}

fn from_py<T: DeserializeOwned>(value: &Bound<'_, PyAny>) -> PyResult<T> {
    let json: String = value
        .py()
        .import("json")?
        .call_method1("dumps", (value,))?
        .extract()?;
    serde_json::from_str(&json).map_err(py_err)
}

fn parse_format(name: &str) -> PyResult<GameFormat> {
    serde_json::from_value(serde_json::Value::String(name.to_string()))
        .map_err(|_| py_err(format!("unknown format {name:?}")))
}

fn parse_player_deck(value: &Bound<'_, PyAny>) -> PyResult<PlayerDeckList> {
    if let Ok(names) = value.extract::<Vec<String>>() {
        return Ok(PlayerDeckList {
            main_deck: names,
            ..PlayerDeckList::default()
        });
    }
    from_py(value)
}

fn validate_seat(
    db: &CardDatabase,
    seat: &str,
    deck: &PlayerDeckList,
    format_config: &FormatConfig,
    match_type: Option<engine::types::match_config::MatchType>,
    player_count: usize,
) -> Result<(), String> {
    validate_name_deck_for_format_full(
        db,
        &deck.main_deck,
        &deck.sideboard,
        &deck.commander,
        &deck.companion,
        &deck.planar_deck,
        &deck.scheme_deck,
        &deck.signature_spell,
        &[],
        format_config,
        match_type,
        player_count,
    )
    .map_err(|reasons| {
        reasons
            .into_iter()
            .map(|reason| format!("{seat} deck: {reason}"))
            .collect::<Vec<_>>()
            .join("; ")
    })
}

#[allow(clippy::too_many_arguments)]
fn start_match(
    db: &CardDatabase,
    player: PlayerDeckList,
    opponent: PlayerDeckList,
    extra_players: Vec<PlayerDeckList>,
    seed: u64,
    format_config: FormatConfig,
    match_config: MatchConfig,
    first_player: Option<u8>,
) -> Result<Box<GameState>, String> {
    let player_count = 2 + extra_players.len();
    if player_count > u8::MAX as usize {
        return Err("too many players".to_string());
    }
    let player_count_u8 = player_count as u8;
    format_config.validate_for_player_count(player_count_u8)?;
    format_config.reject_unimplemented_range_of_influence()?;

    let deck_list = DeckList {
        player,
        opponent,
        ai_decks: extra_players,
        ai_difficulties: Vec::new(),
        draft_set_codes: Vec::new(),
    };

    if !format_config.format.supplies_fixed_deck() {
        validate_seat(
            db,
            "player",
            &deck_list.player,
            &format_config,
            Some(match_config.match_type),
            player_count,
        )?;
        validate_seat(
            db,
            "opponent",
            &deck_list.opponent,
            &format_config,
            Some(match_config.match_type),
            player_count,
        )?;
        for (index, deck) in deck_list.ai_decks.iter().enumerate() {
            validate_seat(
                db,
                &format!("player {}", index + 2),
                deck,
                &format_config,
                Some(match_config.match_type),
                player_count,
            )?;
        }
    }

    let mut state = GameState::new(format_config, player_count_u8, seed);
    state.set_match_config(match_config);

    let payload = resolve_deck_list(db, &deck_list);
    load_and_hydrate_decks(&mut state, &payload, Some(db));
    state.all_card_names = db.card_names().into();

    let empty_seats: Vec<u8> = state
        .players
        .iter()
        .filter(|player| player.library.is_empty())
        .map(|player| player.id.0)
        .collect();
    if !empty_seats.is_empty() {
        return Err(format!(
            "empty library after deck load for seat(s) {empty_seats:?}; \
             pass main_deck names the card database can resolve for every seat"
        ));
    }

    match first_player {
        Some(seat) => {
            start_game_with_starting_player(&mut state, PlayerId(seat));
        }
        None => {
            start_game(&mut state);
        }
    }

    Ok(Box::new(state))
}

fn snapshot_state(state: &mut GameState) -> PersistedGameState {
    state.capture_rng_word_pos();
    PersistedGameState::capture(state.clone())
}

fn restore_match(
    db: &CardDatabase,
    serialized: serde_json::Value,
) -> Result<Box<GameState>, String> {
    let persisted = serde_json::from_value::<PersistedGameState>(serialized)
        .map_err(|error| format!("failed to deserialize GameState: {error}"))?;
    let prepared = persisted
        .prepare_for_restore(PersistedRestoreFinalization::DeferUntilRehydrated)
        .map_err(|error| format!("failed to restore GameState: {error}"))?;
    let state = prepared
        .finalize_after_rehydration(|state| {
            rehydrate_game_from_card_db(state, db);
            Ok(())
        })
        .map_err(|error| format!("failed to restore GameState: {error}"))?;
    Ok(Box::new(state))
}

fn parse_state_value(value: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    if let Ok(json) = value.extract::<String>() {
        return serde_json::from_str(&json).map_err(py_err);
    }
    from_py(value)
}

fn parse_action(value: &Bound<'_, PyAny>) -> PyResult<EngineAction> {
    if let Ok(action) = value.extract::<PyRef<'_, GameAction>>() {
        return Ok(action.inner.clone());
    }
    from_py(value)
}

fn action_kind(action: &EngineAction) -> String {
    format!("{:?}", GameActionKind::from(action))
}

fn action_json(action: &EngineAction) -> PyResult<serde_json::Value> {
    serde_json::to_value(action).map_err(py_err)
}

/// Tagged-union payload (`data`), or `Null` for unit variants such as `PassPriority`.
fn action_data_json(action: &EngineAction) -> PyResult<serde_json::Value> {
    Ok(match action_json(action)? {
        serde_json::Value::Object(mut map) => {
            map.remove("data").unwrap_or(serde_json::Value::Null)
        }
        other => other,
    })
}

/// Build a `CardFace` from MTGJSON atomic card data.
///
/// `mtgjson` should be a dict matching the engine `AtomicCard` shape
/// (`name`, `mana_cost`, `types`, `text`, `layout`, etc.).
/// `oracle_id` is an optional Scryfall oracle ID.
#[pyfunction]
pub fn build_oracle_face(
    mtgjson: Bound<'_, PyAny>,
    oracle_id: Option<String>,
) -> PyResult<Bound<'_, PyAny>> {
    let atomic: AtomicCard = from_py(&mtgjson)?;
    let face = engine_build_oracle_face(&atomic, oracle_id);
    to_py(mtgjson.py(), &face)
}

/// Build a `CardFace` for a multi-face card, skipping MTGJSON keywords.
///
/// See [`build_oracle_face`] for parameter details.
#[pyfunction]
pub fn build_oracle_face_multi(
    mtgjson: Bound<'_, PyAny>,
    oracle_id: Option<String>,
) -> PyResult<Bound<'_, PyAny>> {
    let atomic: AtomicCard = from_py(&mtgjson)?;
    let face = engine_build_oracle_face_multi(&atomic, oracle_id);
    to_py(mtgjson.py(), &face)
}

/// Loaded card database used to create games.
#[pyclass(module = "phase")]
struct Engine {
    db: Arc<CardDatabase>,
}

#[pymethods]
impl Engine {
    /// Load a `card-data.json` export from disk.
    #[staticmethod]
    fn from_path(path: PathBuf) -> PyResult<Self> {
        let db = CardDatabase::from_export(&path).map_err(py_err)?;
        Ok(Self { db: Arc::new(db) })
    }

    /// Load a `card-data.json` export from a JSON string.
    #[staticmethod]
    fn from_json(json: &str) -> PyResult<Self> {
        let db = CardDatabase::from_json_str(json).map_err(py_err)?;
        Ok(Self { db: Arc::new(db) })
    }

    /// Number of card faces in the loaded database.
    fn card_count(&self) -> usize {
        self.db.card_count()
    }

    /// Start a match and return a live [`Game`].
    ///
    /// `player` / `opponent` / `extra_players` may be a list of card names
    /// (treated as the main deck) or a dict matching the engine `PlayerDeckList`
    /// (`main_deck`, `sideboard`, `commander`, ...).
    #[pyo3(signature = (
        player,
        opponent,
        *,
        extra_players = None,
        seed = 42,
        format = "Standard",
        format_config = None,
        match_config = None,
        first_player = None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new_game(
        &self,
        player: Bound<'_, PyAny>,
        opponent: Bound<'_, PyAny>,
        extra_players: Option<Vec<Bound<'_, PyAny>>>,
        seed: u64,
        format: &str,
        format_config: Option<Bound<'_, PyAny>>,
        match_config: Option<Bound<'_, PyAny>>,
        first_player: Option<u8>,
    ) -> PyResult<Game> {
        let player = parse_player_deck(&player)?;
        let opponent = parse_player_deck(&opponent)?;
        let extra_players = extra_players
            .unwrap_or_default()
            .iter()
            .map(parse_player_deck)
            .collect::<PyResult<Vec<_>>>()?;
        let format_config: FormatConfig = match format_config {
            Some(value) => from_py(&value)?,
            None => FormatConfig::for_format(parse_format(format)?).map_err(py_err)?,
        };
        let match_config = match match_config {
            Some(value) => from_py(&value)?,
            None => MatchConfig::default(),
        };
        let state = start_match(
            &self.db,
            player,
            opponent,
            extra_players,
            seed,
            format_config,
            match_config,
            first_player,
        )
        .map_err(py_err)?;
        Ok(Game { state })
    }

    /// Resume a game from a previously exported state.
    ///
    /// `state` may be the dict returned by [`Game::state`], a JSON string of
    /// that dict, or a WASM `TrustedGameStateEnvelope` (`{"state": ...}`).
    fn load_game(&self, state: Bound<'_, PyAny>) -> PyResult<Game> {
        let serialized = parse_state_value(&state)?;
        let state = restore_match(&self.db, serialized).map_err(py_err)?;
        Ok(Game { state })
    }
}

/// An engine `GameAction`. Returned by [`Game::actions`] and accepted by [`Game::apply`].
///
/// The Rust value is held natively — listing and applying actions does not
/// serialize through JSON. Use [`GameAction::to_dict`] only when you need the
/// tagged JSON shape.
#[pyclass(name = "GameAction", module = "phase", frozen)]
#[derive(Clone)]
struct GameAction {
    inner: EngineAction,
}

#[pymethods]
impl GameAction {
    /// Build from the tagged JSON dict (`{"type": "...", "data": ...}`).
    #[staticmethod]
    fn from_dict(value: Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self {
            inner: from_py(&value)?,
        })
    }

    /// Variant name, e.g. `"MulliganDecision"` or `"PassPriority"`.
    #[getter]
    fn kind(&self) -> String {
        action_kind(&self.inner)
    }

    /// Variant payload as a dict (or `None` for unit variants).
    ///
    /// Field names match the engine `GameAction` serde shape, so a `PlayLand`
    /// action exposes `{"object_id": ..., "card_id": ...}`. The same keys are
    /// also available as attributes (`action.object_id`) for debugger inspection.
    #[getter]
    fn data<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &action_data_json(&self.inner)?)
    }

    /// Flattened view of `kind` plus payload fields. Debuggers that inspect
    /// `__dict__` use this instead of the native `EngineAction` layout.
    #[getter]
    fn __dict__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        dict.set_item("kind", action_kind(&self.inner))?;
        match action_data_json(&self.inner)? {
            serde_json::Value::Object(map) => {
                for (key, value) in map {
                    dict.set_item(key, to_py(py, &value)?)?;
                }
            }
            serde_json::Value::Null => {}
            other => {
                dict.set_item("data", to_py(py, &other)?)?;
            }
        }
        Ok(dict)
    }

    /// Tagged JSON dict. Prefer passing this object to [`Game::apply`] instead.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.inner)
    }

    fn __getattr__<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Bound<'py, PyAny>> {
        if !name.starts_with('_') {
            if let serde_json::Value::Object(map) = action_data_json(&self.inner)? {
                if let Some(value) = map.get(name) {
                    return to_py(py, value);
                }
            }
        }
        Err(PyAttributeError::new_err(format!(
            "'GameAction' object has no attribute '{name}'"
        )))
    }

    fn __dir__(&self) -> PyResult<Vec<String>> {
        let mut names = vec![
            "kind".to_string(),
            "data".to_string(),
            "from_dict".to_string(),
            "to_dict".to_string(),
        ];
        if let serde_json::Value::Object(map) = action_data_json(&self.inner)? {
            names.extend(map.keys().cloned());
        }
        names.sort();
        names.dedup();
        Ok(names)
    }

    fn __repr__(&self) -> String {
        let kind = action_kind(&self.inner);
        match action_data_json(&self.inner) {
            Ok(serde_json::Value::Null) => format!("GameAction({kind})"),
            Ok(data) => format!("GameAction({kind}, {data})"),
            Err(_) => format!("GameAction({kind})"),
        }
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, GameAction>>()
            .map(|other| other.inner == self.inner)
            .unwrap_or(false)
    }
}

/// Native result of applying one or more actions.
///
/// Event and prompt values are converted only when their getters are read.
#[pyclass(name = "ActionResult", module = "phase", frozen)]
struct ActionResult {
    inner: EngineActionResult,
    fast_forwarded: usize,
}

#[pymethods]
impl ActionResult {
    /// Events emitted by the requested action and any fast-forwarded passes.
    #[getter]
    fn events<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.inner.events)
    }

    /// Prompt active after all actions represented by this result.
    #[getter]
    fn waiting_for<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.inner.waiting_for)
    }

    /// Game log entries emitted by the represented actions.
    #[getter]
    fn log_entries<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.inner.log_entries)
    }

    /// Number of automatic `PassPriority` actions applied after the requested action.
    #[getter]
    fn fast_forwarded(&self) -> usize {
        self.fast_forwarded
    }

    /// Convert to the engine's JSON-compatible result shape.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.inner)
    }

    fn __repr__(&self) -> String {
        format!(
            "ActionResult(events={}, log_entries={}, fast_forwarded={})",
            self.inner.events.len(),
            self.inner.log_entries.len(),
            self.fast_forwarded
        )
    }
}

/// A started game: inspect legal actions, apply one, and read state.
#[pyclass(module = "phase")]
struct Game {
    state: Box<GameState>,
}

#[pymethods]
impl Game {
    /// Legal actions for the player currently expected to act.
    ///
    /// Returns [`GameAction`] objects. Pass one back to [`Game::apply`] with no
    /// JSON conversion. Dicts in the tagged engine shape are still accepted.
    fn actions(&self) -> Vec<GameAction> {
        legal_actions(&self.state)
            .into_iter()
            .map(|inner| GameAction { inner })
            .collect()
    }

    /// Apply `action` as `actor` (seat index) and return the `ActionResult`.
    ///
    /// `action` should be a [`GameAction`] from [`Game::actions`]. A tagged
    /// JSON dict is still accepted.
    #[pyo3(signature = (actor, action, *, fast_forward = false))]
    fn apply<'py>(
        &mut self,
        actor: u8,
        action: Bound<'py, PyAny>,
        fast_forward: bool,
    ) -> PyResult<ActionResult> {
        let action = parse_action(&action)?;
        let mut result: EngineActionResult =
            apply(&mut self.state, PlayerId(actor), action).map_err(py_err)?;
        let mut fast_forwarded = 0;

        if fast_forward {
            loop {
                let actions = legal_actions(&self.state);
                if actions.len() != 1 || !matches!(actions[0], EngineAction::PassPriority) {
                    break;
                }

                let actor = self.state.priority_player;
                let next =
                    apply(&mut self.state, actor, EngineAction::PassPriority).map_err(py_err)?;
                result.events.extend(next.events);
                result.log_entries.extend(next.log_entries);
                result.waiting_for = next.waiting_for;
                fast_forwarded += 1;
            }
        }

        Ok(ActionResult {
            inner: result,
            fast_forwarded,
        })
    }

    /// Full persisted `GameState` as a Python dict (same serde shape as WASM export).
    ///
    /// Pass the result to [`Engine::load_game`] to resume from this snapshot.
    fn state<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &snapshot_state(&mut self.state))
    }

    /// Current `waiting_for` prompt.
    fn waiting_for<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.state.waiting_for)
    }

    /// Seat that currently holds priority, if any.
    fn priority_player(&self) -> u8 {
        self.state.priority_player.0
    }

    /// Current turn number.
    #[getter]
    fn turn(&self) -> u32 {
        self.state.turn_number
    }

    /// Current phase name.
    #[getter]
    fn phase(&self) -> String {
        format!("{:?}", self.state.phase)
    }

    /// Public player state for `seat`.
    fn player<'py>(&self, py: Python<'py>, seat: u8) -> PyResult<Bound<'py, PyAny>> {
        let player = self
            .state
            .players
            .get(seat as usize)
            .ok_or_else(|| py_err(format!("unknown player seat {seat}")))?;
        to_py(py, player)
    }

    /// Objects currently on the battlefield.
    fn battlefield<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let objects: Vec<_> = self
            .state
            .battlefield
            .iter()
            .filter_map(|id| self.state.objects.get(id))
            .collect();
        to_py(py, &objects)
    }

    /// Objects in `seat`'s hand.
    fn hand<'py>(&self, py: Python<'py>, seat: u8) -> PyResult<Bound<'py, PyAny>> {
        // TODO expose Card class and return list of Cards
        let player = self
            .state
            .players
            .get(seat as usize)
            .ok_or_else(|| py_err(format!("unknown player seat {seat}")))?;
        let objects: Vec<_> = player
            .hand
            .iter()
            .filter_map(|id| self.state.objects.get(id))
            .collect();
        to_py(py, &objects)
    }

    /// Objects in `seat`'s graveyard.
    fn graveyard<'py>(&self, py: Python<'py>, seat: u8) -> PyResult<Bound<'py, PyAny>> {
        let player = self
            .state
            .players
            .get(seat as usize)
            .ok_or_else(|| py_err(format!("unknown player seat {seat}")))?;
        let objects: Vec<_> = player
            .graveyard
            .iter()
            .filter_map(|id| self.state.objects.get(id))
            .collect();
        to_py(py, &objects)
    }

    /// Objects in exile.
    fn exile<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let objects: Vec<_> = self
            .state
            .exile
            .iter()
            .filter_map(|id| self.state.objects.get(id))
            .collect();
        to_py(py, &objects)
    }

    /// Current stack entries.
    fn stack<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.state.stack)
    }

    /// Look up one game object by its numeric object ID.
    fn object<'py>(&self, py: Python<'py>, object_id: u64) -> PyResult<Bound<'py, PyAny>> {
        let object = self
            .state
            .objects
            .get(&ObjectId(object_id))
            .ok_or_else(|| py_err(format!("unknown object id {object_id}")))?;
        to_py(py, object)
    }

    fn __repr__(&self) -> String {
        format!(
            "Game(turn={}, phase={:?}, priority={})",
            self.state.turn_number, self.state.phase, self.state.priority_player.0
        )
    }
}

#[pymodule]
fn phase(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Engine>()?;
    m.add_class::<Game>()?;
    m.add_class::<GameAction>()?;
    m.add_class::<ActionResult>()?;
    m.add_function(wrap_pyfunction!(build_oracle_face, m)?)?;
    m.add_function(wrap_pyfunction!(build_oracle_face_multi, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forest_db() -> CardDatabase {
        CardDatabase::from_json_str(include_str!("../tests/fixtures/forest.json"))
            .expect("forest fixture parses")
    }

    fn sixty_forests() -> PlayerDeckList {
        PlayerDeckList {
            main_deck: vec!["Forest".to_string(); 60],
            ..PlayerDeckList::default()
        }
    }

    #[test]
    fn new_game_lists_and_applies_actions() {
        let db = forest_db();
        let mut state = start_match(
            &db,
            sixty_forests(),
            sixty_forests(),
            Vec::new(),
            1,
            FormatConfig::standard(),
            MatchConfig::default(),
            Some(0),
        )
        .expect("forest game starts");

        let actions = legal_actions(&state);
        assert!(
            !actions.is_empty(),
            "started game should expose at least one action"
        );

        let first = actions[0].clone();
        apply(&mut state, PlayerId(0), first).expect("first legal action applies");
    }

    #[test]
    fn load_game_resumes_from_exported_state() {
        let db = forest_db();
        let mut state = start_match(
            &db,
            sixty_forests(),
            sixty_forests(),
            Vec::new(),
            1,
            FormatConfig::standard(),
            MatchConfig::default(),
            Some(0),
        )
        .expect("forest game starts");
        let first = legal_actions(&state)[0].clone();
        apply(&mut state, PlayerId(0), first).expect("first legal action applies");

        let snapshot = serde_json::to_value(snapshot_state(&mut state)).expect("state serializes");
        let restored = restore_match(&db, snapshot).expect("state restores");

        assert_eq!(restored.turn_number, state.turn_number);
        assert_eq!(restored.priority_player, state.priority_player);
        assert_eq!(restored.waiting_for, state.waiting_for);
        assert_eq!(legal_actions(&restored), legal_actions(&state));
    }

    #[test]
    fn action_data_json_exposes_variant_payload() {
        assert_eq!(
            action_data_json(&EngineAction::PassPriority).unwrap(),
            serde_json::Value::Null
        );

        let play_land = EngineAction::PlayLand {
            object_id: ObjectId(99),
            card_id: engine::types::identifiers::CardId(42),
        };
        let data = action_data_json(&play_land).unwrap();
        assert_eq!(data["object_id"], 99);
        assert_eq!(data["card_id"], 42);
    }
}
