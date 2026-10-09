"""PLAN-fael-onoff: a trial's own database (issue 01M4GY6AFF).

Each tracked *.env.example that names DATABASE_URL gets a .env beside it on a
database only this trial uses: the template's dev values, blanks filled so the
app's env check starts. Never the developer's own .env — its secrets and its
database stay out of the trial. The workspace and the evaluator each get one;
both are dropped when the harness exits.
"""
import atexit
import json
import os
import re
import subprocess
from urllib.parse import urlsplit

import replica

NAME = re.compile(r"^onoff_[0-9a-f]+_(ws|ev)$")
# ponytail: dev S3 (rustfs) stays shared — trials only add objects to it


def write_env(root, name):
    """.env beside each template in root, on database name; drop it at exit.
    Returns the DATABASE_URLs written."""
    assert NAME.match(name), name
    urls = []
    for ex in replica.paths(root, "ls-files", "*.env.example"):
        text = open(os.path.join(root, ex)).read()
        if not re.search(r"^DATABASE_URL=", text, re.M):
            continue
        out = []
        for line in text.replace("<N>", "9").splitlines():
            k, eq, v = line.partition("=")
            if eq and re.fullmatch(r"[A-Z][A-Z0-9_]*", k):
                if k == "DATABASE_URL":
                    v = urlsplit(v)._replace(path="/" + name).geturl()
                    urls.append(v)
                line = f"{k}={v or 'onoff-placeholder'}"
            out.append(line)
        open(os.path.join(root, os.path.dirname(ex), ".env"), "w").write("\n".join(out) + "\n")
    for u in urls:
        atexit.register(drop, u)
    return urls


def reset(root, env):
    """Every package's `db:reset` (create, migrate, seed); {package: exit}."""
    done = {}
    for pj in replica.paths(root, "ls-files", "package.json", "*/package.json"):
        if "db:reset" in json.load(open(os.path.join(root, pj))).get("scripts", {}):
            d = os.path.dirname(pj)
            done[d] = subprocess.run(["bun", "run", "db:reset"], cwd=os.path.join(root, d),
                                     env=env, capture_output=True).returncode
    return done


def drop(url):
    u = urlsplit(url)
    name = u.path[1:]
    assert NAME.match(name), name
    admin = u._replace(path="/postgres").geturl()
    subprocess.run(["bun", "-e", "import {SQL} from 'bun'; const s = new SQL(Bun.env.ADMIN);"
                    f"await s.unsafe('drop database if exists {name} with (force)');"
                    "await s.close()"],
                   env=os.environ | {"ADMIN": admin}, capture_output=True)
