import json
from importlib.resources import files

MESSAGE = json.loads(files(__package__).joinpath("message.json").read_text())["message"]
