from phase import Agent, Engine

engine = Engine.from_path("fixtures/forest.json")
game = engine.new_game(
    player=["Forest"] * 60,
    opponent=["Forest"] * 60,
    seed=42,
    first_player=0,
)

actions = game.actions()
game, result = game.apply(0, actions[0])

saved = game.state()
resumed = engine.load_game(saved)
actions = resumed.actions()

agent = Agent("Easy")
choice = agent.choose_action(game, 0)
attackers = agent.choose_attackers(game, 0)
blockers = agent.choose_blockers(game, 0)
score = agent.evaluate_state(game, 0)
