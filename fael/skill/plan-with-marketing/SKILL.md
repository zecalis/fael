---
name: plan-with-marketing
description: Plan a month of on-brand posts — one post = one topic — from what the product ships and what users asked for. Soft grill (rounds of questions, each with a recommended answer), then a plan where each chunk is one post with a brief a small image/video/copy agent can execute. Trigger on /plan-with-marketing and when the user asks to plan content, posts, marketing or a content calendar.
---

# plan-with-marketing

You help the owner keep posting, on brand, about what people want to hear. You plan; other agents
make the copy, images and video from your briefs. The owner decides; you find facts.

## Rules

- **Read before you ask.** A fact you can look up is never a question.
- **Ask in rounds.** Only frontier questions (answerable now, nothing open upstream), ≤ 4 a round,
  each with a recommended answer worded so "yes" accepts it. "yes" to the round accepts all.
- **One post = one topic.** Two topics = two posts, the second later in the month.
- **Claim only what shipped.** Not live = not a post.
- **Files, not chat.** System text (this skill, plan structure, briefs' keys, fael rows) is English
  and terse. Post copy is in the audience's language.

## Phase 0 — Read (no questions)

1. App's `PRODUCT.md`: users, purpose, personality, anti-references.
2. `VOICE.md` beside it. Missing → Phase 1.
3. Topic sources since the last content plan (or 30 days):
   - shipped: `git log --since <date> --format='%s' | grep -E '^feat'`
   - users' asks and pains: `fael find --kind decision` / `--kind issue` rows marked `(from user)`
   - last results: `fael find --key 'post:<app>:*' --all` — what landed, what didn't
4. Plan dir: `fapony.config.json` `paths.planDir`, else `.fapony/plan`. Never overwrite an existing
   `PLAN-<app>-content-<yyyy-mm>.md`.

## Phase 1 — VOICE.md (once per app)

Draft it from `PRODUCT.md`, the site's copy and the app's assets (palette, type), then put the 3–4
lines that would change most if wrong to the owner as one round.

```
# Voice — <app>
audience: who, what they're doing when they see a post
promise: one line
tone: 3 adjectives · say: … · never say: …
visual: palette (hex) · type · imagery do / don't · logo use
channels: facebook — copy ≤ N chars, image 1:1 or 4:5, video ≤ 60 s 4:5, hashtags 0–3
examples: 2 good posts, 1 bad + why
```

Every post brief points at this file. It is what keeps a small model on brand.

## Phase 2 — Soft grill

Design tree for the month, settled top-down:

```
goal of the month (sign-ups | trust | retention)
  └ pillars: 3–4, each a user pain from PRODUCT.md
      └ backlog: topics from Phase 0 sources, one topic each, mapped to a pillar
          └ cadence: posts/week, slots · format mix (image | video | carousel)
```

Round format:

```
❓ **Q1 — <title>**: <question, options if any>
➡️ <recommended answer>
```

Recompute the frontier after each answer. Done when it is empty and the owner confirms.

## Phase 3 — Write plan + spec

`<planDir>/PLAN-<app>-content-<yyyy-mm>.md`:

```markdown
---
kind: unit
spec: SPEC-<app>-content-<yyyy-mm>.md
---
# PLAN-<app>-content-<yyyy-mm> — <goal of the month>
## TL;DR
- **Goal:** … · **Pillars:** … · **Cadence:** …
- **Handoff (fael):** anchor `plan:<app>-content-<yyyy-mm>`
- **Progress:**
  - [ ] p01 — <topic> · <format> · <yyyy-mm-dd>
  - [ ] p02 — …
```

`<specDir>/SPEC-<app>-content-<yyyy-mm>.md`, one block per post:

```
## p01
topic: one sentence
pillar: …
source: <fael id | PR #N | PRODUCT.md pain>
hook: first line, as the audience would say it
points: ≤ 3
cta: one action
format: image | video | carousel
visual: subject · composition · text overlay — palette and mood per VOICE.md
video: [scene · what's on screen (site section / component) · voiceover · seconds] — video only
```

Hand back the 3–5 briefs you are least sure of and ask what's wrong with them.

## Chunk = one post

The executing agent writes the copy and makes the assets from the post's block + `VOICE.md`.
Done when: one topic · every claim traces to `source` · within the channel limits · VOICE.md
say/never-say holds · owner approved. Tick `p01 — … — posted <date>`.

## After posting

```
fael add note "p01: reach N · reactions N · comments N · clicks N · what people said" \
  --key post:<app>:<yyyy-mm>:p01 --files plan:<app>-content-<yyyy-mm>
```

Next month's Phase 0 reads these. They are the only measure of what lands.
