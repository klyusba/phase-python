"""
Python bindings for the phase.rs Magic: The Gathering rules engine.
"""

from collections.abc import Sequence
from os import PathLike
from typing import Any, final

@final
class ActionResult:
    """
    Native result of applying one or more actions.
    
    Event and prompt values are converted only when their getters are read.
    """
    def __repr__(self, /) -> str: ...
    @property
    def events(self, /) -> Any:
        """
        Events emitted by the requested action and any fast-forwarded passes.
        """
    @property
    def fast_forwarded(self, /) -> int:
        """
        Number of automatic `PassPriority` actions applied after the requested action.
        """
    @property
    def log_entries(self, /) -> Any:
        """
        Game log entries emitted by the represented actions.
        """
    def to_dict(self, /) -> Any:
        """
        Convert to the engine's JSON-compatible result shape.
        """
    @property
    def waiting_for(self, /) -> Any:
        """
        Prompt active after all actions represented by this result.
        """

@final
class Engine:
    """
    Loaded card database used to create games.
    """
    def card_count(self, /) -> int:
        """
        Number of card faces in the loaded database.
        """
    @staticmethod
    def from_json(json: str) -> Engine:
        """
        Load a `card-data.json` export from a JSON string.
        """
    @staticmethod
    def from_path(path: str |PathLike[str]) -> Engine:
        """
        Load a `card-data.json` export from disk.
        """
    def load_game(self, /, state: Any) -> Game:
        """
        Resume a game from a previously exported state.
        
        `state` may be the dict returned by [`Game::state`], a JSON string of
        that dict, or a WASM `TrustedGameStateEnvelope` (`{"state": ...}`).
        """
    def new_game(self, /, player: Any, opponent: Any, *, extra_players: Sequence[Any] |None = None, seed: int = 42, format: str = "Standard", format_config: Any |None = None, match_config: Any |None = None, first_player: int |None = None) -> Game:
        """
        Start a match and return a live [`Game`].
        
        `player` / `opponent` / `extra_players` may be a list of card names
        (treated as the main deck) or a dict matching the engine `PlayerDeckList`
        (`main_deck`, `sideboard`, `commander`, ...).
        """

@final
class Game:
    """
    A started game: inspect legal actions, apply one, and read state.
    """
    def __repr__(self, /) -> str: ...
    def actions(self, /) -> list[GameAction]:
        """
        Legal actions for the player currently expected to act.
        
        Returns [`GameAction`] objects. Pass one back to [`Game::apply`] with no
        JSON conversion. Dicts in the tagged engine shape are still accepted.
        """
    def apply(self, /, actor: int, action: Any, *, fast_forward: bool = False) -> ActionResult:
        """
        Apply `action` as `actor` (seat index) and return the `ActionResult`.
        
        `action` should be a [`GameAction`] from [`Game::actions`]. A tagged
        JSON dict is still accepted.
        """
    def battlefield(self, /) -> Any:
        """
        Objects currently on the battlefield.
        """
    def exile(self, /) -> Any:
        """
        Objects in exile.
        """
    def graveyard(self, /, seat: int) -> Any:
        """
        Objects in `seat`'s graveyard.
        """
    def hand(self, /, seat: int) -> Any:
        """
        Objects in `seat`'s hand.
        """
    def object(self, /, object_id: int) -> Any:
        """
        Look up one game object by its numeric object ID.
        """
    @property
    def phase(self, /) -> str:
        """
        Current phase name.
        """
    def player(self, /, seat: int) -> Any:
        """
        Public player state for `seat`.
        """
    def priority_player(self, /) -> int:
        """
        Seat that currently holds priority, if any.
        """
    def stack(self, /) -> Any:
        """
        Current stack entries.
        """
    def state(self, /) -> Any:
        """
        Full persisted `GameState` as a Python dict (same serde shape as WASM export).
        
        Pass the result to [`Engine::load_game`] to resume from this snapshot.
        """
    @property
    def turn(self, /) -> int:
        """
        Current turn number.
        """
    def waiting_for(self, /) -> Any:
        """
        Current `waiting_for` prompt.
        """

@final
class GameAction:
    """
    An engine `GameAction`. Returned by [`Game::actions`] and accepted by [`Game::apply`].
    
    The Rust value is held natively — listing and applying actions does not
    serialize through JSON. Use [`GameAction::to_dict`] only when you need the
    tagged JSON shape.
    """
    @property
    def __dict__(self, /) -> dict:
        """
        Flattened view of `kind` plus payload fields. Debuggers that inspect
        `__dict__` use this instead of the native `EngineAction` layout.
        """
    def __dir__(self, /) -> list[str]: ...
    def __eq__(self, other: object, /) -> bool: ...
    def __getattr__(self, name: str, /) -> Any: ...
    def __repr__(self, /) -> str: ...
    @property
    def data(self, /) -> Any:
        """
        Variant payload as a dict (or `None` for unit variants).
        
        Field names match the engine `GameAction` serde shape, so a `PlayLand`
        action exposes `{"object_id": ..., "card_id": ...}`. The same keys are
        also available as attributes (`action.object_id`) for debugger inspection.
        """
    @staticmethod
    def from_dict(value: Any) -> GameAction:
        """
        Build from the tagged JSON dict (`{"type": "...", "data": ...}`).
        """
    @property
    def kind(self, /) -> str:
        """
        Variant name, e.g. `"MulliganDecision"` or `"PassPriority"`.
        """
    def to_dict(self, /) -> Any:
        """
        Tagged JSON dict. Prefer passing this object to [`Game::apply`] instead.
        """

def build_oracle_face(mtgjson: Any, oracle_id: str |None) -> Any:
    """
    Build a `CardFace` from MTGJSON atomic card data.
    
    `mtgjson` should be a dict matching the engine `AtomicCard` shape
    (`name`, `mana_cost`, `types`, `text`, `layout`, etc.).
    `oracle_id` is an optional Scryfall oracle ID.
    """

def build_oracle_face_multi(mtgjson: Any, oracle_id: str |None) -> Any:
    """
    Build a `CardFace` for a multi-face card, skipping MTGJSON keywords.
    
    See [`build_oracle_face`] for parameter details.
    """
