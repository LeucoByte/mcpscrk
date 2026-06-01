// mcpscrk - workbench frontend logic.
//
// Talks to the JSON API exposed by the Rust binary. Keeps a small client-side
// model (materials, inventory, blueprint) and renders it. The estimate is
// computed locally with BigInt for instant feedback.

(function () {
  "use strict";

  // Profile fields, grouped by set. Mirrors the catalog on the Rust side.
  // No example values: empty inputs make it obvious what is filled and what is not.
  const FIELD_GROUPS = [
    { num: "1", name: "Identity", fields: [
      ["firstname", "First name"], ["lastnames", "Last names"], ["nicknames", "Nicknames"],
      ["usernames", "Usernames"], ["emails", "Emails"], ["ids", "IDs"], ["phones", "Phones"],
    ]},
    { num: "2", name: "Relations", fields: [
      ["partners", "Partners"], ["children", "Children"], ["pets", "Pets"],
      ["parents", "Parents"], ["siblings", "Siblings"],
    ]},
    { num: "3", name: "Passions", fields: [
      ["teams", "Teams"], ["athletes", "Athletes"], ["sports", "Sports"], ["artists", "Artists"],
      ["movies", "Movies"], ["games", "Games"], ["hobbies", "Hobbies"], ["cars", "Cars"],
    ]},
    { num: "4", name: "Context", fields: [
      ["cities", "Cities"], ["places", "Places"], ["companies", "Companies"], ["jobtitles", "Job titles"],
      ["projects", "Projects"], ["words", "Words"], ["nationalities", "Nationalities"],
      ["faithterms", "Faith terms"], ["zodiac", "Zodiac"],
    ]},
    { num: "5", name: "Numeric", fields: [
      ["dates", "Dates"], ["numbers", "Numbers"], ["postcodes", "Postcodes"],
    ]},
  ];

  // Permanent symbol blocks that can be edited in place (pencil), but not deleted.
  const EDITABLE_SYMBOLS = ["Separator", "Special Char", "All Symbols"];
  // Blocks whose first slot is "" (optional — loop may contribute nothing).
  const NULL_CHOICE_BLOCKS = ["Digit", "Separator", "Special Char", "All Symbols"];

  /** CSV token for the null blueprint slot (contribute nothing in that loop). */
  function formatCsvValue(v) {
    return v === "" ? '""' : v;
  }
  function formatPeekValue(v) {
    return v === "" ? '""' : v;
  }

  const state = {
    materials: [],
    inventory: [],
    blueprint: [], // ordered list of block names
    selected: null, // selected material key
    dragIndex: null, // blueprint drag source index
    wordlistPath: null, // uploaded wordlist path on the server
    hashMode: null, // selected hashcat mode for cracking
    editingSymbols: null, // symbol block currently open in the editor
    engines: null, // { hashcat: bool, john: bool } availability
    wordlistEntries: 0, // entries in the attached wordlist
    crackTimer: null, // status-poll timer handle
    detection: null, // { candidates, john, source } from last detect run
  };

  // --- tiny helpers ---------------------------------------------------------

  const $ = (sel) => document.querySelector(sel);
  const el = (tag, cls) => {
    const node = document.createElement(tag);
    if (cls) node.className = cls;
    return node;
  };

  async function api(path, method, body) {
    const opts = { method: method || "GET", headers: { "Content-Type": "application/json" } };
    if (body !== undefined) opts.body = JSON.stringify(body);
    const res = await fetch(path, opts);
    return res.json();
  }

  // Group digits in threes for readability without losing BigInt precision.
  function groupDigits(str) {
    return str.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  }

  // --- tabs -----------------------------------------------------------------

  function setupTabs() {
    const tabs = document.querySelectorAll(".tab");
    const panels = document.querySelectorAll(".panel");
    tabs.forEach((tab) => {
      tab.addEventListener("click", () => {
        tabs.forEach((t) => t.classList.toggle("active", t === tab));
        panels.forEach((p) => p.classList.toggle("active", p.id === tab.dataset.tab));
      });
    });
  }

  // --- profile --------------------------------------------------------------

  function renderProfileFields() {
    const root = $("#profile-sets");
    root.innerHTML = "";
    FIELD_GROUPS.forEach((group) => {
      const box = el("div", "set-group");

      const head = el("div", "set-head");
      const badge = el("span", "set-badge");
      badge.textContent = group.num;
      const name = el("span", "set-name");
      name.textContent = group.name;
      const fill = el("span", "set-fill");
      fill.id = "fill_" + group.num;
      fill.textContent = `0/${group.fields.length}`;
      head.append(badge, name, fill);
      box.appendChild(head);

      const fields = el("div", "fields");
      group.fields.forEach(([key, label]) => {
        const field = el("div", "set-field");
        field.id = "field_" + key;
        const lab = el("label");
        lab.textContent = label;
        const input = el("input");
        input.type = "text";
        input.id = "f_" + key;
        // Enforce lowercase, no spaces: capitalization is the engine's job, and
        // values are comma-separated, so a literal space is never wanted.
        input.addEventListener("input", () => {
          const cleaned = input.value.toLowerCase().replace(/\s+/g, "");
          if (input.value !== cleaned) input.value = cleaned;
          field.classList.toggle("filled", input.value.trim() !== "");
          updateSummary();
        });
        field.append(lab, input);
        fields.appendChild(field);
      });
      box.appendChild(fields);
      root.appendChild(box);
    });
    updateSummary();
  }

  // Live count of filled fields, per group and in total.
  function updateSummary() {
    let total = 0;
    FIELD_GROUPS.forEach((group) => {
      let filled = 0;
      group.fields.forEach(([k]) => {
        if ($("#f_" + k).value.trim() !== "") filled++;
      });
      total += filled;
      const fill = $("#fill_" + group.num);
      fill.textContent = `${filled}/${group.fields.length}`;
      fill.classList.toggle("has", filled > 0);
    });
    $("#profile-summary").innerHTML = `<b>${total}</b> field(s) filled across ${FIELD_GROUPS.length} sets`;
  }

  function collectFields() {
    const fields = {};
    FIELD_GROUPS.forEach((g) => g.fields.forEach(([k]) => (fields[k] = $("#f_" + k).value)));
    return fields;
  }

  async function updateProfile() {
    state.materials = await api("/api/profile", "POST", { fields: collectFields() });
    if (state.selected && !state.materials.some((m) => m.key === state.selected)) {
      state.selected = null;
      $("#lab-source").textContent = "none selected";
    }
    renderMaterials();
    // The fixed Date block is auto-derived from the profile, so refresh it.
    refreshInventory();
    $("#profile-status").textContent = state.materials.length
      ? `Materials updated: ${state.materials.length} available.`
      : "No materials yet - fill in some fields.";
  }

  function clearProfile() {
    FIELD_GROUPS.forEach((g) => g.fields.forEach(([k]) => {
      $("#f_" + k).value = "";
      $("#field_" + k).classList.remove("filled");
    }));
    updateSummary();
    $("#profile-status").textContent = "Cleared. Press Update to apply.";
  }

  // --- materials ------------------------------------------------------------

  function renderMaterials() {
    const list = $("#materials-list");
    list.innerHTML = "";
    if (!state.materials.length) {
      const li = el("li");
      li.className = "empty-note";
      li.textContent = "No materials. Fill the profile and update.";
      list.appendChild(li);
      return;
    }
    state.materials.forEach((m) => {
      const li = el("li");
      if (state.selected === m.key) li.classList.add("selected");

      const main = el("div", "mat-main");
      main.innerHTML = `<span class="mat-key">${m.key}</span><span class="mat-cat">${m.category}</span>`;
      main.addEventListener("click", () => selectMaterial(m));

      const count = el("span", "mat-count");
      count.textContent = m.count;
      const info = el("button", "mat-info");
      info.textContent = "info";
      info.title = "Show values";
      info.addEventListener("click", () => showMaterialPeek(m.key));

      li.append(main, count, info);
      list.appendChild(li);
    });
  }

  function capitalizeFirst(s) {
    return s ? s.charAt(0).toUpperCase() + s.slice(1) : s;
  }

  function selectMaterial(m) {
    state.selected = m.key;
    $("#lab-source").textContent = `${m.key} (${m.count})`;
    $("#block-name").placeholder = `auto: ${capitalizeFirst(m.key)}`;
    if (m.sample && m.sample.length) $("#test-word").value = m.sample[0];
    renderMaterials();
  }

  function currentCap() {
    const r = document.querySelector('input[name="cap"]:checked');
    return r ? r.value : "minimal";
  }

  // --- blocks ---------------------------------------------------------------

  async function refreshInventory() {
    const resp = await api("/api/blocks");
    state.inventory = resp.blocks;
    renderInventory();
    renderBlueprint();
  }

  async function createBlock() {
    if (!state.selected) return flashReport("Select a material first.", true);
    const resp = await api("/api/block", "POST", {
      name: $("#block-name").value,
      source: state.selected,
      cap: currentCap(),
      leet: $("#leet").checked,
    });
    state.inventory = resp.blocks;
    renderInventory();
    if (resp.error) flashReport(resp.error, true);
    $("#block-name").value = "";
  }

  async function deleteBlock(name) {
    const resp = await api("/api/block/delete", "POST", { name });
    state.inventory = resp.blocks;
    state.blueprint = state.blueprint.filter((n) => n !== name);
    renderInventory();
    renderBlueprint();
  }

  function blockByName(name) {
    return state.inventory.find((b) => b.name === name);
  }

  function renderInventory() {
    const root = $("#inventory");
    root.innerHTML = "";
    if (!state.inventory.length) {
      const note = el("div", "empty-note");
      note.textContent = "No pieces yet. Craft one in the modifier lab.";
      root.appendChild(note);
      return;
    }
    state.inventory.forEach((b) => {
      const piece = el("div", b.fixed ? "piece fixed" : "piece");
      const name = el("span", "p-name");
      name.textContent = b.name;
      const count = el("span", "p-count");
      count.textContent = b.count;
      piece.append(name, count);

      piece.appendChild(pieceButton("+ add", "Add to blueprint", () => addToBlueprint(b.name)));
      piece.appendChild(pieceButton("info", "Show first values", () => showBlockPeek(b.name)));

      // The three symbol blocks are editable in place via a pencil; Date and
      // Digit are fixed; crafted blocks can be deleted.
      if (EDITABLE_SYMBOLS.includes(b.name)) {
        piece.appendChild(pieceButton("edit", "Edit characters", () => openSpecialsEditor(b.name)));
      } else if (!b.fixed) {
        const del = pieceButton("x", "Delete piece", () => deleteBlock(b.name));
        del.classList.add("del");
        piece.appendChild(del);
      }
      root.appendChild(piece);
    });
  }

  function pieceButton(text, title, onClick) {
    const b = el("button", "pbtn");
    b.textContent = text;
    b.title = title;
    b.addEventListener("click", onClick);
    return b;
  }

  // --- blueprint ------------------------------------------------------------

  function addToBlueprint(name) {
    state.blueprint.push(name);
    renderBlueprint();
  }
  function removeFromBlueprint(i) {
    state.blueprint.splice(i, 1);
    renderBlueprint();
  }
  function move(i, delta) {
    const j = i + delta;
    if (j < 0 || j >= state.blueprint.length) return;
    [state.blueprint[i], state.blueprint[j]] = [state.blueprint[j], state.blueprint[i]];
    renderBlueprint();
  }
  function reorder(from, to) {
    if (from === to || from == null) return;
    const [item] = state.blueprint.splice(from, 1);
    state.blueprint.splice(to, 0, item);
    renderBlueprint();
  }

  function renderBlueprint() {
    const root = $("#blueprint");
    root.innerHTML = "";
    if (!state.blueprint.length) {
      const note = el("div", "empty-note");
      note.textContent = "Empty. Add pieces from the inventory, then drag to order them.";
      root.appendChild(note);
      updateEstimate();
      return;
    }
    state.blueprint.forEach((name, i) => {
      const loop = el("div", "loop");
      loop.draggable = true;
      loop.dataset.index = String(i);

      const label = el("div", "loop-label");
      label.textContent = `Loop ${i + 1}`;
      const nm = el("div", "loop-name");
      const b = blockByName(name);
      nm.textContent = `${name} (${b ? b.count : "?"})`;

      const ctl = el("div", "loop-ctl");
      ctl.append(
        ctlButton("<", "Move left", () => move(i, -1)),
        ctlButton(">", "Move right", () => move(i, 1)),
        ctlButton("info", "Show first values", () => showBlockPeek(name)),
        ctlButton("x", "Remove", () => removeFromBlueprint(i)),
      );

      loop.append(label, nm, ctl);
      wireDrag(loop, i);
      root.appendChild(loop);

      if (i < state.blueprint.length - 1) {
        const arrow = el("div", "loop-arrow");
        arrow.textContent = "->";
        root.appendChild(arrow);
      }
    });
    updateEstimate();
  }

  function ctlButton(text, title, onClick) {
    const b = el("button", "btn mini ghost");
    b.textContent = text;
    b.title = title;
    b.draggable = false;
    b.addEventListener("click", (e) => { e.stopPropagation(); onClick(); });
    return b;
  }

  function wireDrag(loop, index) {
    loop.addEventListener("dragstart", (e) => {
      state.dragIndex = index;
      loop.classList.add("dragging");
      e.dataTransfer.effectAllowed = "move";
    });
    loop.addEventListener("dragend", () => {
      loop.classList.remove("dragging");
      document.querySelectorAll(".loop.drop-target").forEach((n) => n.classList.remove("drop-target"));
      state.dragIndex = null;
    });
    loop.addEventListener("dragover", (e) => {
      e.preventDefault();
      loop.classList.add("drop-target");
    });
    loop.addEventListener("dragleave", () => loop.classList.remove("drop-target"));
    loop.addEventListener("drop", (e) => {
      e.preventDefault();
      reorder(state.dragIndex, index);
    });
  }

  // Estimate = product of block sizes, computed with BigInt.
  function updateEstimate() {
    let total = state.blueprint.length ? 1n : 0n;
    for (const name of state.blueprint) {
      const b = blockByName(name);
      total *= BigInt(b ? b.count : 0);
    }
    $("#estimate").textContent = groupDigits(total.toString());
  }

  // --- info popups ----------------------------------------------------------

  async function showBlockPeek(name) {
    const resp = await api("/api/block/peek", "POST", { name, limit: 50 });
    openModal(resp);
  }
  async function showMaterialPeek(key) {
    const resp = await api("/api/material/peek", "POST", { key, limit: 50 });
    openModal(resp);
  }
  function openModal(resp) {
    $("#modal-title").textContent = `${resp.name} - ${resp.count} value(s), first ${resp.values.length}`;
    let body = resp.values.map(formatPeekValue).join("\n") || "(empty)";
    if (NULL_CHOICE_BLOCKS.includes(resp.name)) {
      body +=
        "\n\n—\nAlso contemplates null case (\"\"): this loop may add nothing and is included in the forge.";
    }
    $("#modal-body").textContent = body;
    $("#modal").classList.remove("hidden");
  }
  function closeModal() {
    $("#modal").classList.add("hidden");
  }

  // --- single-value expansion -----------------------------------------------

  async function testWord() {
    const word = $("#test-word").value.trim();
    if (!word) return;
    const params = new URLSearchParams({
      word,
      source: state.selected || "",
      cap: currentCap(),
      leet: $("#leet").checked,
    });
    const resp = await api("/api/expand?" + params.toString());
    $("#expand-output").textContent = `${resp.count} variant(s)\n\n` + resp.variants.join("\n");
  }

  // --- preview / forge ------------------------------------------------------

  function parseBound(raw, fallback) {
    const s = String(raw).trim();
    if (s === "" || s === "-") return fallback;
    const n = parseInt(s, 10);
    return Number.isFinite(n) && n >= 0 ? n : fallback;
  }

  function boundsLabel(min, max) {
    const lo = min === 0 ? "-" : String(min);
    const hi = max === 0 ? "-" : String(max);
    return `[${lo}..${hi}]`;
  }

  function bounds() {
    return {
      min: parseBound($("#len-min").value, 0),
      max: parseBound($("#len-max").value, 0),
    };
  }
  function currentMode() {
    const r = document.querySelector('input[name="mode"]:checked');
    return r ? r.value : "overwrite";
  }

  async function runPreview() {
    if (!state.blueprint.length) return flashReport("Blueprint is empty.", true);
    const { min, max } = bounds();
    const resp = await api("/api/preview", "POST", { order: state.blueprint, min, max, limit: 50 });
    const s = resp.stats;
    let header =
      `# preview - ${resp.lines.length} shown\n` +
      `# generated=${s.generated} emitted=${s.emitted} filtered=${s.filtered} duplicates=${s.duplicates} type=${s.kind}\n`;
    if (s.emitted === 0 && s.generated > 0) {
      header += `# all ${groupDigits(String(s.generated))} candidates fell outside length ${boundsLabel(min, max)} - widen the range.\n`;
    }
    $("#preview-output").textContent = header + "\n" + resp.lines.join("\n");
  }

  async function runForge() {
    if (!state.blueprint.length) return flashReport("Blueprint is empty.", true);
    const { min, max } = bounds();
    const resp = await api("/api/forge", "POST", {
      order: state.blueprint, min, max, mode: currentMode(), path: $("#out-path").value,
    });
    if (resp.error) return flashReport(resp.error, true);
    const s = resp.stats;
    const headline = s.emitted === 0
      ? `<span class="err">0 entries written - all ${groupDigits(String(s.generated))} candidates fell outside length ${boundsLabel(min, max)}. Widen the range.</span>`
      : `<span class="ok">${groupDigits(String(s.emitted))} entries written to ${escapeHtml(resp.path)}</span>`;
    $("#forge-report").innerHTML =
      headline +
      `<table>` +
      `<tr><td class="k">List count</td><td>${groupDigits(String(s.emitted))}</td></tr>` +
      `<tr><td class="k">Type</td><td>${s.kind}</td></tr>` +
      `<tr><td class="k">Generated</td><td>${groupDigits(String(s.generated))}</td></tr>` +
      `<tr><td class="k">Filtered out</td><td>${groupDigits(String(s.filtered))}</td></tr>` +
      `<tr><td class="k">Duplicates</td><td>${groupDigits(String(s.duplicates))}</td></tr>` +
      `</table>`;
  }

  function flashReport(msg, isError) {
    $("#forge-report").innerHTML = `<span class="${isError ? "err" : "ok"}">${msg}</span>`;
  }

  function downloadWordlist() {
    if (!state.blueprint.length) {
      flashReport("Nothing to download yet — add blocks to the blueprint and forge first.", true);
      return;
    }
    const path = $("#out-path").value.trim();
    if (!path) {
      flashReport("Set a file name before downloading.", true);
      return;
    }
    window.open("/api/download?path=" + encodeURIComponent(path), "_blank");
  }

  // --- symbol blocks editor -------------------------------------------------

  async function openSpecialsEditor(name) {
    state.editingSymbols = name;
    $("#specials-title").textContent = "Edit " + name;
    const resp = await api("/api/block/peek", "POST", { name, limit: 200 });
    $("#specials-input").value = resp.values.map(formatCsvValue).join(",");
    $("#specials-modal").classList.remove("hidden");
  }
  function closeSpecialsModal() {
    $("#specials-modal").classList.add("hidden");
  }
  async function saveSpecials(values) {
    if (!state.editingSymbols) return;
    const resp = await api("/api/specials", "POST", { name: state.editingSymbols, values });
    state.inventory = resp.blocks;
    renderInventory();
    renderBlueprint();
    closeSpecialsModal();
  }

  // --- cracking lab ---------------------------------------------------------

  async function loadEngines() {
    const e = await api("/api/crack/engines");
    state.engines = { hashcat: !!e.hashcat, john: !!e.john };

    // Reflect availability on each radio: disable the missing ones and tag them.
    setEngineOption("hashcat", e.hashcat);
    setEngineOption("john", e.john);

    const parts = [];
    parts.push(e.hashcat ? "hashcat <b>ready</b>" : "hashcat <b>missing</b>");
    parts.push(e.john ? "john <b>ready</b>" : "john <b>missing</b>");
    $("#engine-status").innerHTML = parts.join(" &middot; ");

    // Default selection: prefer hashcat, fall back to whichever is installed.
    const warn = $("#engine-warn");
    if (e.hashcat) {
      selectEngine("hashcat");
      warn.innerHTML = e.john ? "" : `<span class="warn">john is not installed - hashcat selected by default.</span>`;
    } else if (e.john) {
      selectEngine("john");
      warn.innerHTML = `<span class="warn">hashcat is not installed - john selected by default.</span>`;
    } else {
      const checked = document.querySelector('input[name="engine"]:checked');
      if (checked) checked.checked = false;
      warn.innerHTML = `<span class="err">No cracking engine installed.</span> You can craft and download wordlists, but cracking is disabled until hashcat or john is on PATH.`;
    }
    renderHashCandidates();
  }

  // Enable/disable an engine radio and annotate its label.
  function setEngineOption(name, available) {
    const label = $("#engine-label-" + name);
    const radio = label.querySelector("input");
    const text = label.querySelector("span");
    radio.disabled = !available;
    label.classList.toggle("disabled", !available);
    text.textContent = available ? name : `${name} (not installed)`;
  }

  function selectEngine(name) {
    const radio = $("#engine-label-" + name).querySelector("input");
    radio.checked = true;
  }

  // Map common hashcat modes to john format names (fallback when john list is sparse).
  const HC_TO_JOHN = {
    0: "raw-md5", 100: "raw-sha1", 1400: "raw-sha256", 1700: "raw-sha512",
    1000: "nt", 3200: "bcrypt", 1800: "sha512crypt", 500: "md5crypt",
  };

  function renderHashCandidates() {
    const engine = currentEngine();
    const select = $("#hash-candidates");
    select.innerHTML = "";
    if (!state.detection) {
      const opt = document.createElement("option");
      opt.value = "";
      opt.textContent = "Run detection first";
      select.appendChild(opt);
      return;
    }

    if (engine === "john") {
      // John's own format guesses drive the dropdown when john is selected.
      const names = [...state.detection.john];
      // Enrich with john equivalents of hashcat candidates if missing.
      state.detection.candidates.forEach((c) => {
        if (c.mode !== null && HC_TO_JOHN[c.mode]) {
          const jf = HC_TO_JOHN[c.mode];
          if (!names.some((n) => n.toLowerCase() === jf.toLowerCase())) {
            names.push(jf);
          }
        }
      });
      if (!names.length) {
        const opt = document.createElement("option");
        opt.value = "";
        opt.textContent = "No john formats detected - try another hash";
        select.appendChild(opt);
        return;
      }
      names.forEach((name) => {
        const opt = document.createElement("option");
        opt.value = "john:" + name;
        opt.textContent = name;
        select.appendChild(opt);
      });
    } else {
      state.detection.candidates.forEach((c) => {
        const opt = document.createElement("option");
        opt.value = c.mode === null ? "" : "hc:" + c.mode;
        opt.textContent = c.mode === null ? c.name : `${c.mode} - ${c.name}`;
        select.appendChild(opt);
      });
    }
    select.selectedIndex = 0;
  }

  function parseHashSelection() {
    const val = $("#hash-candidates").value;
    if (!val) return { mode: null, john_format: null };
    if (val.startsWith("john:")) return { mode: null, john_format: val.slice(5) };
    if (val.startsWith("hc:")) return { mode: parseInt(val.slice(3), 10), john_format: null };
    return { mode: null, john_format: null };
  }

  function updateDetectDisplay() {
    const select = $("#hash-candidates");
    const det = state.detection;
    if (!det) return;

    const parts = [];
    if (state.engines && state.engines.hashcat && det.candidates.length) {
      const src = det.source === "hashcat" ? "hashcat --identify" : "built-in table";
      parts.push(`<b>${src}:</b> ${det.candidates.length} candidate(s)`);
    }
    if (det.john && det.john.length) {
      parts.push(`<b>john:</b> ${det.john.length} format guess(es)`);
    }
    const sel = select.options[0] ? select.options[0].textContent : "none";
    $("#detect-status").innerHTML =
      (parts.length ? parts.join(" &middot; ") : "No detection results") +
      ` — selected: <b>${escapeHtml(sel)}</b>`;

    const lines = [];
    if (state.engines && state.engines.hashcat && det.candidates.length) {
      const hc = det.candidates.slice(0, 8).map((c) =>
        c.mode !== null ? `${c.mode} - ${c.name}` : c.name
      ).map(escapeHtml).join(", ");
      lines.push(`<span class="muted">hashcat sees:</span> ${hc}`);
    }
    if (det.john && det.john.length) {
      lines.push(`<span class="muted">john sees:</span> ${det.john.slice(0, 10).map(escapeHtml).join(", ")}`);
    }
    $("#detect-john").innerHTML = lines.length ? lines.join("<br>") : "";
  }

  async function detectHash() {
    const hash = $("#crack-hash").value.trim();
    if (!hash) return;
    $("#detect-status").textContent = "Detecting...";
    $("#detect-john").textContent = "";
    const resp = await api("/api/crack/detect", "POST", { hash });
    state.detection = {
      candidates: resp.candidates,
      john: resp.john || [],
      source: resp.source,
    };
    renderHashCandidates();
    updateDetectDisplay();
  }

  function openCrackStation() {
    const hash = $("#crack-hash").value.trim();
    if (hash && navigator.clipboard) {
      navigator.clipboard.writeText(hash).catch(() => {});
    }
    window.open("https://crackstation.net/", "_blank", "noopener");
  }

  function currentEngine() {
    const r = document.querySelector('input[name="engine"]:checked');
    return r ? r.value : null;
  }

  function setupDropzone() {
    const zone = $("#dropzone");
    const input = $("#wordlist-file");
    zone.addEventListener("click", () => input.click());
    input.addEventListener("change", () => {
      if (input.files.length) uploadWordlist(input.files[0]);
    });
    ["dragenter", "dragover"].forEach((ev) =>
      zone.addEventListener(ev, (e) => { e.preventDefault(); zone.classList.add("drag"); }));
    ["dragleave", "drop"].forEach((ev) =>
      zone.addEventListener(ev, (e) => { e.preventDefault(); zone.classList.remove("drag"); }));
    zone.addEventListener("drop", (e) => {
      const files = e.dataTransfer.files;
      if (files && files.length) uploadWordlist(files[0]);
    });
  }

  async function uploadWordlist(file) {
    const label = $("#dropzone-label");
    label.textContent = `Uploading ${file.name}...`;
    const form = new FormData();
    form.append("file", file);
    const res = await fetch("/api/crack/upload", { method: "POST", body: form });
    const resp = await res.json();
    if (resp.error || !resp.path) {
      label.textContent = resp.error || "Upload failed.";
      return;
    }
    state.wordlistPath = resp.path;
    state.wordlistEntries = resp.lines;
    $("#dropzone").classList.add("loaded");
    const unit = resp.lines === 1 ? "entry" : "entries";
    label.textContent = `${file.name} - ${resp.lines.toLocaleString()} ${unit} loaded`;
  }

  async function runCrack() {
    const engine = currentEngine();
    if (!engine) {
      return setCrackStatus("No cracking engine available. Install hashcat or john to crack.", true);
    }
    if (state.engines && !state.engines[engine]) {
      return setCrackStatus(`${engine} is not installed - pick an installed engine.`, true);
    }
    const hash = $("#crack-hash").value.trim();
    if (!hash) return setCrackStatus("Paste a hash first.", true);
    if (!state.wordlistPath) return setCrackStatus("Attach a wordlist first.", true);

    const sel = parseHashSelection();
    if (engine === "hashcat" && sel.mode === null) {
      return setCrackStatus("Select a hashcat mode first (run detection).", true);
    }
    if (engine === "john" && !sel.john_format) {
      return setCrackStatus("Select a john format first (run detection).", true);
    }

    const resp = await api("/api/crack/start", "POST", {
      hash, engine, mode: sel.mode, john_format: sel.john_format, wordlist: state.wordlistPath,
    });
    if (!resp.ok) {
      return setCrackStatus(resp.error || "Could not start.", true);
    }

    crackRunningUI(true);
    setCrackStatus("Running... polling progress.", false);
    $("#crack-verdict").textContent = "Running...";
    $("#crack-log").textContent = "";
    $("#crack-log").classList.add("hidden");
    pollCrack();
  }

  async function cancelCrack() {
    setCrackStatus("Cancelling...", true);
    await api("/api/crack/cancel", "POST", {});
  }

  function crackRunningUI(running) {
    $("#crack-run").disabled = running;
    // The cancel button only exists while an attack is in flight.
    $("#crack-cancel").classList.toggle("hidden", !running);
    $("#crack-cancel").disabled = !running;
    $("#crack-progress").classList.toggle("hidden", !running);
  }

  function scrollCrackLog() {
    const log = $("#crack-log");
    log.scrollTop = log.scrollHeight;
  }

  function pollCrack() {
    if (state.crackTimer) clearTimeout(state.crackTimer);
    const tick = async () => {
      let j;
      try {
        j = await api("/api/crack/status");
      } catch (e) {
        state.crackTimer = setTimeout(tick, 800);
        return;
      }
      updateCrackProgress(j);
      if (j.log) {
        $("#crack-log").textContent = j.log;
        $("#crack-log").classList.remove("hidden");
        scrollCrackLog();
      }
      if (j.finished) {
        crackRunningUI(false);
        finishCrack(j);
        return;
      }
      state.crackTimer = setTimeout(tick, 700);
    };
    tick();
  }

  function updateCrackProgress(j) {
    const pct = Math.max(0, Math.min(100, j.percent || 0));
    $("#crack-bar").style.width = pct.toFixed(1) + "%";
    const done = (j.done || 0).toLocaleString();
    const total = (j.total || 0).toLocaleString();
    const eng = j.engine ? j.engine : "engine";
    $("#crack-progress-text").textContent =
      `${eng} - ${done} / ${total} (${pct.toFixed(1)}%) - ${j.elapsed_secs || 0}s elapsed`;
  }

  function finishCrack(j) {
    if (j.note) setCrackStatus(j.note, false);
    if (j.error) {
      setCrackStatus(j.error, true);
      $("#crack-verdict").innerHTML = `<span class="err">Stopped:</span> ${escapeHtml(j.error)}`;
      return;
    }
    if (j.cracked) {
      setCrackStatus("Password recovered.", false);
      renderVerdict(j);
    } else {
      setCrackStatus("Not found. Design a better build and try again.", true);
      $("#crack-verdict").innerHTML =
        `<span class="err">NOT FOUND</span> - the wordlist did not contain it. Go back to reconnaissance or reorder the blueprint.`;
    }
  }

  function renderVerdict(j) {
    const v = j.verdict;
    let html = `<div class="verdict-plain">Plaintext: ${escapeHtml(j.plaintext || "")}</div>`;
    if (v) {
      html +=
        `<div class="verdict-score">${v.score} / 10.0 &middot; ${escapeHtml(v.profile)}</div>` +
        `<p class="hint">${escapeHtml(v.why)}</p>`;
    }
    $("#crack-verdict").innerHTML = html;
  }

  function escapeHtml(s) {
    return s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
  }

  function setCrackStatus(msg, isError) {
    const node = $("#crack-status");
    node.textContent = msg;
    node.style.color = isError ? "var(--red-bright)" : "var(--phosphor)";
  }

  // --- wire up --------------------------------------------------------------

  function init() {
    setupTabs();
    renderProfileFields();
    renderMaterials();
    renderInventory();
    renderBlueprint();

    $("#profile-update").addEventListener("click", updateProfile);
    $("#profile-clear").addEventListener("click", clearProfile);
    $("#block-create").addEventListener("click", createBlock);
    $("#test-run").addEventListener("click", testWord);
    $("#preview-run").addEventListener("click", runPreview);
    $("#forge-run").addEventListener("click", runForge);
    $("#download-run").addEventListener("click", downloadWordlist);
    $("#modal-close").addEventListener("click", closeModal);
    $("#modal").addEventListener("click", (e) => { if (e.target.id === "modal") closeModal(); });

    // Special chars editor.
    $("#specials-cancel").addEventListener("click", closeSpecialsModal);
    $("#specials-save").addEventListener("click", () => saveSpecials($("#specials-input").value));
    $("#specials-reset").addEventListener("click", () => saveSpecials(""));

    // Cracking lab.
    $("#detect-run").addEventListener("click", detectHash);
    $("#crackstation-run").addEventListener("click", openCrackStation);
    $("#crack-run").addEventListener("click", runCrack);
    $("#crack-cancel").addEventListener("click", cancelCrack);
    setupDropzone();
    loadEngines();
    document.querySelectorAll('input[name="engine"]').forEach((r) => {
      r.addEventListener("change", () => {
        renderHashCandidates();
        updateDetectDisplay();
      });
    });

    // Load the fixed blocks (Date, Digit, symbols…) on startup.
    refreshInventory();
  }

  document.addEventListener("DOMContentLoaded", init);
})();
