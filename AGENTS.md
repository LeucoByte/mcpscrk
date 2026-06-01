# AGENTS.md — read this first

You are an AI assistant who just inherited `mcpscrk`. This file is the handoff:
context, mental model, how to run it, the full HTTP API with copy-paste `curl`
examples, and the exact end-to-end test the previous agent used. Read it before
touching anything.

---

## 1. What this project is (and is not)

`mcpscrk` is an **educational OSINT wordlist workbench**, written in Rust. It is
an "artisan's bench": a human feeds in *publicly available* facts about a target
(a name, a birthday, a pet, a favourite team...) and the tool helps them
**assemble candidate passwords** from those facts, then optionally **test** the
result against a hash with real cracking tools.

The whole point is pedagogical: to make it visceral how predictable
"personal" passwords are. `Elliot1986#` is not a secret if everyone knows the
target is named Elliot and was born in 1986. (Examples in this doc use the
fictitious identity Elliot Alderson - never real personal data.)

What it is **not**:

- It is **not** a hash cracker. We never reinvent hashcat or John the Ripper.
  Detection and cracking are *delegated* to those tools when installed. Our job
  is to build great wordlists and orchestrate the external tools nicely.
- It is **not** a mass-exploitation tool. It is a teaching instrument.

Design philosophy (please preserve it): **clean, elegant, simple, fast, fully
commented and documented English code.** The UI is deliberately calm and
professional — a tidy workshop, "Mr. Robot / Elliot Alderson" aesthetic (deep
blacks/greys, a single fsociety-red accent, amber for counts, faint phosphor
green for generated output). **No emojis anywhere.**

---

## 2. The mental model (the pipeline)

```
Profile (OSINT facts)  ->  Materials  ->  Blocks  ->  Blueprint  ->  Forge -> wordlist.txt -> Crack
```

- **Profile**: raw OSINT fields grouped into 6 semantic sets (Identity,
  Relations, Passions, Context, Numeric, Special). Each field is a
  comma-separated, lowercase list. See `FIELD_GROUPS` in `web/app.js` and
  `engine/sets.rs`.
- **Materials**: the non-empty profile fields, offered as raw ingredients.
- **Blocks** (`engine/block.rs`): an "assembly piece" — a de-duplicated set of
  strings built from a material **plus rules**: capitalization mode (`exact`,
  `minimal`, `matrix`) and optional leet. The `Dates` material runs through the
  date engine (`engine/dates.rs`).
- **Permanent blocks** (always present, never craftable, never deletable):
  - `Dates` — auto-derived from the profile `dates` field; refreshes on profile update.
  - `Separators` — editable, defaults `.` `-` `_`.
  - `Special Chars` — editable, the common symbols that are **not** separators.
  - `All Symbols` — editable, the union of both.
  The three symbol blocks are editable in place (pencil icon); empty input
  restores their defaults. See `engine/sets.rs` constants + `server/state.rs`.
- **Blueprint**: an ordered list of block names. Order = nested loops. `[A][B][C]`
  emits `a+b+c` for every combination. Reorder by drag-and-drop in the UI.
- **Forge** (`engine/forge.rs`): lazy odometer iteration over the blueprint, with
  a min/max length filter, de-duplication, and streaming to a file (overwrite or
  append). Default output path is `/tmp/wordlist.txt`.
- **Crack** (`crack/`): detect the hash type (hashcat `--identify`, with a
  built-in fallback table), then run hashcat or john against an uploaded
  wordlist, then rate the recovered plaintext.

---

## 3. Layout

```
src/
  main.rs              entry point; warns if not root (cracking tools may need it)
  cli.rs               --port flag
  engine/
    sets.rs            profile fields, categories, symbol-block defaults, CSV parse
    expand.rs          capitalization modes + leet (with combinatorial guards)
    dates.rs           flexible date parsing -> numeric variants
    block.rs           Block = material + rules -> de-duplicated values
    blueprint.rs       estimated_size()
    filters.rs         LengthFilter
    forge.rs           generate / preview / forge (streaming), ForgeStats
  crack/
    detect.rs          hash type detection (hashcat --identify + fallback)
    runner.rs          run hashcat / john as subprocesses, with timeout
    rating.rs          score the recovered plaintext (Profile + why)
  server/
    mod.rs             axum server bootstrap
    state.rs           Workshop (profile, inventory, permanent blocks) behind a Mutex
    routes.rs          all HTTP routes + DTOs
web/
  index.html  style.css  app.js   (embedded into the binary via include_str!)
```

**Important:** the frontend is embedded with `include_str!`. After editing
anything in `web/`, you **must `cargo build`** for the change to be served.

---

## 4. Build & run

```bash
cargo build
cargo run -- --port 8787      # then open http://localhost:8787
```

### Startup behaviour (engine probing)

On launch the binary:

- warns if **not root** (building still works; only the lab may need privileges);
- probes `hashcat` and `john` on `PATH` and logs one of:
  - both present -> `cracking engines ready: hashcat + john (default: hashcat).`
  - only one -> warns that the lab will use that one;
  - neither -> warns you can **craft but NOT crack**.

The UI mirrors this: a missing engine's radio is disabled and tagged
`(not installed)`. Default selection is **hashcat** if present, else **john**,
else none (with a clear warning). `runCrack` refuses to run an uninstalled engine.

### Installing the engines (read the John caveat)

```bash
# hashcat + a CPU OpenCL runtime (pocl). hashcat works out of the box from apt.
sudo apt install -y hashcat pocl-opencl-icd
```

**John caveat:** the apt `john` package is **core** John 1.9.0, which only knows a
handful of crypt formats (descrypt, md5crypt, bcrypt, LM...) and **cannot crack
raw hashes** like `raw-md5`. You need **jumbo** John. There is no apt package, so
build it from source (this is what the previous agent did and it works):

```bash
sudo apt install -y build-essential libssl-dev git
git clone --depth 1 https://github.com/openwall/john.git /tmp/john
cd /tmp/john/src && ./configure && make -j"$(nproc)"
# Install to a persistent dir and shadow the core binary via a wrapper:
sudo mkdir -p /opt/john && sudo cp -r /tmp/john/run/* /opt/john/
sudo sh -c 'printf "#!/bin/sh\nexec /opt/john/john \"\$@\"\n" > /usr/local/bin/john && chmod +x /usr/local/bin/john'
john --list=formats | tr "," "\n" | grep -i raw-md5   # should print Raw-MD5
```

A bare symlink does NOT work: John resolves its home from the invocation path and
fails with "Cannot find John home" — use the wrapper above so it execs the full
`/opt/john/john` path. Also, `/opt/john` is root-owned, so the runner passes
`--session=<tmp>` (and `--pot=<tmp>`) to keep John's `.log/.rec/.pot` in a writable
place; see `crack/runner.rs::run_john`.

---

## 5. HTTP API (with curl examples)

Base URL assumes `--port 8787`. All bodies are JSON unless noted.

### Profile & materials

```bash
# Store OSINT fields; returns the resulting materials.
curl -s localhost:8787/api/profile -H 'content-type: application/json' \
  -d '{"fields":{"firstname":"elliot","dates":"09/09/1986"}}'

# List materials / peek raw values.
curl -s localhost:8787/api/materials
curl -s localhost:8787/api/material/peek -H 'content-type: application/json' \
  -d '{"key":"firstname","limit":50}'

# Try the expansion of a single word (source matters for dates).
curl -s 'localhost:8787/api/expand?word=elliot&source=firstname'
curl -s 'localhost:8787/api/expand?word=09/09/1986&source=dates'
```

### Blocks (inventory)

```bash
# Craft a block. cap in {exact,minimal,matrix}; leet bool.
curl -s localhost:8787/api/block -H 'content-type: application/json' \
  -d '{"name":"Firstname","source":"firstname","cap":"minimal","leet":false}'

curl -s localhost:8787/api/blocks
curl -s localhost:8787/api/block/peek -H 'content-type: application/json' \
  -d '{"name":"Firstname","limit":50}'
curl -s localhost:8787/api/block/delete -H 'content-type: application/json' \
  -d '{"name":"Firstname"}'

# Edit a permanent symbol block (name = Separators | Special Chars | All Symbols).
# Empty values restores that block's defaults.
curl -s localhost:8787/api/specials -H 'content-type: application/json' \
  -d '{"name":"Special Chars","values":"!,@,#,$"}'
```

### Metrics, preview, forge, download

```bash
# Estimated total + per-block sizes for an ordered blueprint.
curl -s localhost:8787/api/metrics -H 'content-type: application/json' \
  -d '{"order":["Firstname","Dates","Special Chars"]}'

# First N candidates, no disk write.
curl -s localhost:8787/api/preview -H 'content-type: application/json' \
  -d '{"order":["Firstname","Dates","Special Chars"],"min":1,"max":64,"limit":20}'

# Write the wordlist. mode in {overwrite,append}.
curl -s localhost:8787/api/forge -H 'content-type: application/json' \
  -d '{"order":["Firstname","Dates","Special Chars"],"min":1,"max":64,"mode":"overwrite","path":"/tmp/wordlist.txt"}'

# Download a generated file.
curl -s 'localhost:8787/api/download?path=/tmp/wordlist.txt' -o out.txt
```

### Cracking lab

The attack runs **asynchronously**: `start` kicks it off, then you **poll**
`status` for live progress, and `cancel` kills it. Only one job at a time.

```bash
# Which engines are installed?
curl -s localhost:8787/api/crack/engines           # {"hashcat":bool,"john":bool}

# Detect candidate hash types. Returns hashcat's structural candidates
# (re-ordered so common modes lead -> first one is the UI default) PLUS John's
# independent opinion as a cross-check.
curl -s localhost:8787/api/crack/detect -H 'content-type: application/json' \
  -d '{"hash":"e3684bdaff51e48f8c9e294dd23e64cb"}'
# -> {"candidates":[{"mode":0,"name":"MD5"},...],"source":"hashcat","john":["LM",...]}

# Upload a wordlist (multipart). Returns a server-side path (and entry count).
curl -s localhost:8787/api/crack/upload -F 'file=@/tmp/wordlist.txt'

# Start an attack. engine in {hashcat,john}; mode is the hashcat mode (0=MD5).
# If the chosen engine fails to RUN (bad flag/version), it auto-falls back to the
# other installed engine (see job.rs). Returns {"ok":true}.
curl -s localhost:8787/api/crack/start -H 'content-type: application/json' \
  -d '{"hash":"e3684bdaff51e48f8c9e294dd23e64cb","engine":"hashcat","mode":0,"wordlist":"/tmp/wordlist.txt"}'

# Poll progress (the UI polls this ~every 0.7s).
curl -s localhost:8787/api/crack/status

# Cancel the running job (SIGKILLs the engine process).
curl -s -X POST localhost:8787/api/crack/cancel
```

`status` (the `CrackJob` snapshot) looks like:

```json
{ "running": false, "finished": true, "cracked": true,
  "plaintext": "Elliot1986#", "engine": "hashcat",
  "done": 234, "total": 234, "percent": 100.0, "elapsed_secs": 2,
  "log": "...", "error": null, "note": null,
  "verdict": { "score": 6.5, "profile": "Careful", "why": "..." } }
```

---

## 6. The canonical end-to-end test (the demo)

This is the reference scenario. It proves the full pipeline and is the fastest
sanity check after changes.

- **Target hash (MD5):** `e3684bdaff51e48f8c9e294dd23e64cb`
- **Known plaintext (the lesson):** `Elliot1986#`
  Verify with: `printf '%s' 'Elliot1986#' | md5sum`
- **OSINT facts:** firstname `elliot`, date `09/09/1986`.
- **Blueprint:** `Firstname` (cap `minimal`) + `Dates` + `Special Chars`.
  - `minimal` firstname yields `elliot / ELLIOT / Elliot`.
  - the date engine yields `1986` (among others).
  - `Special Chars` contains `#`.
  - So the forge produces `Elliot` + `1986` + `#` = `Elliot1986#`. 

Scripted version:

```bash
PORT=8787
HASH=e3684bdaff51e48f8c9e294dd23e64cb
curl -s localhost:$PORT/api/profile -H 'content-type: application/json' \
  -d '{"fields":{"firstname":"elliot","dates":"09/09/1986"}}' >/dev/null
curl -s localhost:$PORT/api/block -H 'content-type: application/json' \
  -d '{"name":"Firstname","source":"firstname","cap":"minimal","leet":false}' >/dev/null
curl -s localhost:$PORT/api/forge -H 'content-type: application/json' \
  -d '{"order":["Firstname","Dates","Special Chars"],"min":1,"max":64,"mode":"overwrite","path":"/tmp/wordlist.txt"}'
# Detect -> should report MD5 (mode 0)
curl -s localhost:$PORT/api/crack/detect -H 'content-type: application/json' -d "{\"hash\":\"$HASH\"}"
# Crack -> should recover Elliot1986#  (start, then poll status; see Cracking lab above)
curl -s localhost:$PORT/api/crack/start -H 'content-type: application/json' \
  -d "{\"hash\":\"$HASH\",\"engine\":\"hashcat\",\"mode\":0,\"wordlist\":\"/tmp/wordlist.txt\"}"
curl -s localhost:$PORT/api/crack/status
```

If hashcat is unavailable, `grep -Fxq 'Elliot1986#' /tmp/wordlist.txt` still proves
the wordlist build is correct; only the cracking step needs the external tool.

---

## 7. Gotchas & conventions (don't relearn these the hard way)

- **Everything in English** — code, comments, docs, UI. Non-negotiable.
- **No emojis** anywhere in code or UI.
- **Rebuild after `web/` edits** (assets are `include_str!`-embedded).
- **Combinatorial explosion** is real: `matrix` capitalization and combinatorial
  leet are guarded by `COMBINATORIAL_CAP` in `engine/expand.rs`. Keep guards.
- **Permanent blocks** (`Dates`, `Separators`, `Special Chars`, `All Symbols`)
  are reserved: `create_block` rejects those names; they never appear as
  materials; only the three symbol blocks are editable (pencil), never deletable.
- **Hashcat install pain:** the distro `.deb` extracted by hand tends to miss
  OpenCL kernels/`libminizip`. Use `sudo apt install` + an OpenCL ICD (`pocl`)
  rather than hand-extracting packages. The previous agent burned a lot of time
  here; don't repeat it.
- **State is in-memory** (`Workshop` behind a `Mutex`); restarting the binary
  resets the profile and inventory to defaults. There is no persistence yet.
- **Download is gated client-side**: the button refuses to download when the
  blueprint is empty (nothing has been forged).
