# phase-python

Unofficial Python bindings for the [phase.rs](https://github.com/phase-rs/phase) Magic: The Gathering rules engine.

```python
from phase import Engine, Game, GameAction
```

Install from PyPI (prebuilt wheels; no Rust toolchain required):

```bash
pip install phase-python
```

The import name is `phase`. The distribution is `phase-python` because `phase` is already taken on PyPI.

## Card data

The engine needs a `card-data.json` oracle export. After installing the package, generate one with:

```bash
phase-gen -o card-data.json
```

Or generate `card-data.json` for one set only:
```bash
phase-gen --set SET -o card-data.json
```

If `AtomicCards.json.gz` is missing, `phase-gen` downloads it from [MTGJSON](https://mtgjson.com). You can also pass an existing dump:

```bash
phase-gen -i AtomicCards.json.gz -o card-data.json
```

You can generate a `card-data.json` for single set with

```bash
phase-gen -i HOB.json -o card-data.json
```

A hosted snapshot is available at https://data.phase-rs.dev/card-data.json.

## Quick start

```python
from phase import Engine

engine = Engine.from_path("card-data.json")
game = engine.new_game(
    player=["Forest"] * 60,
    opponent=["Forest"] * 60,
    seed=42,
    first_player=0,
)

actions = game.actions()
result = game.apply(0, actions[0])
```

See [API.md](API.md) for the full Python API.

## Develop from source

Requires Rust (see `rust-toolchain.toml`) and Python 3.10+.

```bash
uv venv && source .venv/bin/activate
uv pip install maturin
maturin develop --generate-stubs
```

## License

MIT
