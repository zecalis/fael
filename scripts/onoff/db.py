"""PLAN-fael-onoff: a trial's own database server (issue 01M4GY6AFF).

Each trial starts a throwaway Postgres container, removed at exit with every
database in it — the agent's, the evaluator's and any the agent makes itself
(a repo rule like "one database per worktree"). Each tracked *.env.example
that names DATABASE_URL gets a .env beside it pointing at that container: the
template's dev values, blanks filled so the app's env check starts. Never the
developer's own .env or database server.
"""
import atexit
import json
import os
import re
import subprocess
import time
from urllib.parse import urlsplit

import replica

IMAGE = "postgres:17"  # vela's infra/vela/docker-compose.yml
# ponytail: dev S3 (rustfs) stays shared — trials only add objects to it
# ponytail: the dev server (5433) stays reachable — an agent that ignores its
# .env can still reach it; a container per whole trial closes that


def server(run_id):
    """Start the trial's Postgres on a free local port; returns the port."""
    name = f"onoff-{run_id}"
    subprocess.run(["docker", "run", "-d", "--rm", "--name", name, "-e", "POSTGRES_USER=vela",
                    "-e", "POSTGRES_PASSWORD=vela", "-p", "127.0.0.1::5432", IMAGE],
                   check=True, capture_output=True)
    atexit.register(subprocess.run, ["docker", "rm", "-f", name], capture_output=True)
    for _ in range(60):
        if subprocess.run(["docker", "exec", name, "pg_isready", "-U", "vela", "-h", "127.0.0.1"],
                          capture_output=True).returncode == 0:
            break
        time.sleep(1)
    else:
        raise RuntimeError(f"{name}: postgres not ready after 60s")
    out = subprocess.run(["docker", "port", name, "5432/tcp"], check=True,
                         capture_output=True, text=True).stdout
    return int(out.split()[0].rsplit(":", 1)[1])


def write_env(root, port, name):
    """.env beside each template in root, on database name at the trial's
    server. Returns the DATABASE_URLs written."""
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
                    u = urlsplit(v)
                    v = u._replace(netloc=f"{u.username}:{u.password}@127.0.0.1:{port}",
                                   path="/" + name).geturl()
                    urls.append(v)
                line = f"{k}={v or 'onoff-placeholder'}"
            out.append(line)
        open(os.path.join(root, os.path.dirname(ex), ".env"), "w").write("\n".join(out) + "\n")
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
