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
actions = resumed.actions()
