# phase Python API

PyO3 bindings for the [phase.rs](https://github.com/phase-rs/phase) Magic: The Gathering rules engine (`phase-engine`).

```python
from phase import Engine, Game, GameAction
```

## Install

Requires Rust (the same nightly pin as phase: see `rust-toolchain.toml`) and Python 3.10+.

```bash
uv venv && source .venv/bin/activate
uv pip install maturin
maturin develop
```

You need a `card-data.json` export from the phase repo (`./scripts/gen-card-data.sh`).
Download from https://data.phase-rs.dev/card-data.json.

## Quick start

```python
from phase import Engine

engine = Engine.from_path("fixtures/forest.json")
game = engine.new_game(
    player=["Forest"] * 60,
    opponent=["Forest"] * 60,
    seed=42,
    first_player=0,
)

actions = game.actions()
result = game.apply(0, actions[0])

saved = game.state()
resumed = engine.load_game(saved)
```

## `Engine`

Loaded card database. Create games from decks or from a saved snapshot.

### `Engine.from_path(path) -> Engine`

Load a `card-data.json` export from disk.

### `Engine.from_json(json: str) -> Engine`

Load the same export from a JSON string.

### `Engine.card_count() -> int`

Number of card faces in the loaded database.

### `Engine.new_game(player, opponent, *, extra_players=None, seed=42, format="Standard", format_config=None, match_config=None, first_player=None) -> Game`

Start a match: resolve decks against the card database, hydrate libraries, then run `start_game` (or `start_game_with_starting_player` if `first_player` is set).

**Decks** (`player`, `opponent`, and each entry in `extra_players`) may be:

- a `list[str]` of card names (treated as `main_deck`)
- a dict matching the engine `PlayerDeckList`:

| Field | Default |
| --- | --- |
| `main_deck` | required if using a dict |
| `sideboard` | `[]` |
| `commander` | `[]` |
| `companion` | `[]` |
| `attraction_deck` | `[]` |
| `planar_deck` | `[]` |
| `scheme_deck` | `[]` |
| `contraption_deck` | `[]` |
| `sticker_sheets` | `[]` |
| `signature_spell` | `[]` |
| `bracket_tier` | engine default (`Core`) |

Seat 0 is `player`, seat 1 is `opponent`, further seats are `extra_players` (player count is `2 + len(extra_players)`). Names that do not resolve in the card database are skipped; an empty library after load is an error.

Decks are validated for the chosen format unless the format supplies a fixed deck (Momir).

**Keyword arguments**

| Name | Meaning |
| --- | --- |
| `seed` | RNG seed (`u64`, default `42`) |
| `format` | Engine `GameFormat` name, e.g. `"Standard"`, `"Modern"`, `"Commander"` |
| `format_config` | Optional dict matching engine `FormatConfig` (overrides `format`) |
| `match_config` | Optional dict matching engine `MatchConfig` |
| `first_player` | Optional seat index. If set, skip the CR 103.1 die roll and start that player |

`format` strings are the serde names of `GameFormat`: `Standard`, `Limited`, `Commander`, `Pioneer`, `Modern`, `Premodern`, `Legacy`, `Vintage`, `Historic`, `Timeless`, `Pauper`, `PauperCommander`, `DuelCommander`, `TinyLeaders`, `Oathbreaker`, `Brawl`, `HistoricBrawl`, `FreeForAll`, `TwoHeadedGiant`, `Archenemy`, `Planechase`, `Momir`.

### `Engine.load_game(state) -> Game`

Resume from a previously exported snapshot. `state` may be:

- the dict returned by `Game.state()`
- a JSON string of that dict
- a WASM trusted envelope (`{"state": ...}`)

Restore deserializes through `PersistedGameState`, rehydrates printed cards from this engine’s database, rebuilds combat declaration display, fast-forwards the RNG to the captured offset, and recovers an orphaned Resolve All latch if present.

Use the same (or compatible) card database that produced the snapshot.

## `Game`

A live match. The engine `GameState` is held in Rust; only snapshots and prompts are converted to Python dicts.

### `Game.actions() -> list[GameAction]`

Legal actions for the player currently expected to act. Returns native `GameAction` objects (no JSON). Pass one to `apply`.

### `Game.apply(actor: int, action) -> dict`

Apply `action` as seat `actor` (trusted seat index, `0` / `1` / …).

`action` should be a `GameAction` from `actions()`. A tagged JSON dict (`{"type": "...", "data": ...}`) is still accepted.

Returns an `ActionResult` dict:

| Key | Meaning |
| --- | --- |
| `events` | list of `GameEvent` dicts |
| `waiting_for` | current prompt after the action |
| `log_entries` | omitted when empty |

Raises `ValueError` if the action is illegal for that actor.

### `Game.state() -> dict`

Full persistence snapshot (same serde shape as the WASM export, including RNG high-water). Pass to `Engine.load_game`. This clones and serializes the whole `GameState`; it is the expensive path.

### `Game.waiting_for() -> dict`

Current `WaitingFor` prompt (tagged JSON).

### `Game.priority_player() -> int`

Seat that currently holds priority.

## `GameAction`

Frozen wrapper around the engine `GameAction` enum. Listing and applying actions copies the Rust value; it does not round-trip JSON.

```python
action = game.actions()[0]
action.kind          # e.g. "MulliganDecision", "PassPriority"
game.apply(0, action)
```

| Member | Meaning |
| --- | --- |
| `kind` | Variant name |
| `to_dict()` | Tagged JSON dict (`{"type": "...", "data": ...}`). Use only when you need JSON |
| `GameAction.from_dict(d)` | Build from that tagged dict |
| `==` | Compares the underlying engine action |

`repr` is `GameAction(<kind>)`.

## Errors

Engine and decode failures surface as `ValueError` with a message (invalid action, unknown format, deck validation, empty library, restore failure, and so on).

## Notes

- `actor` in `apply` is the seat submitting the action, not a value taken from the action payload.
- Mana abilities are omitted from the flat `actions()` list (same as the engine `legal_actions` helper).
- `Game.state()` / `load_game` are for save/restore. The hot loop is `actions()` → pick a `GameAction` → `apply`.
