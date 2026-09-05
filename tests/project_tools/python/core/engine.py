import json

def load_settings():
    # Notice we read from /app! That is where ORE mounts the source code.
    with open("/app/config/settings.json", "r") as f:
        return json.load(f)

def process_template(user: str, score: int):
    with open("/app/data/template.txt", "r") as f:
        template = f.read()
    return template.replace("{user}", user).replace("{score}", str(score))