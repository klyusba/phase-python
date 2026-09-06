"""Python bindings for the phase.rs Magic: The Gathering rules engine."""

from phase._phase import (
    ActionResult,
    Engine,
    Game,
    GameAction,
    build_oracle_face,
    build_oracle_face_multi,
)

__all__ = [
    "ActionResult",
    "Engine",
    "Game",
    "GameAction",
    "build_oracle_face",
    "build_oracle_face_multi",
]
