/* comline-doc-html: the version picker, over VERSIONS above. No network:
   the data is inlined, so it works from file:// too. */
(function () {
  var body = document.body;
  var current = body.getAttribute("data-version");
  var page = body.getAttribute("data-page") || "index.html";
  var root = body.getAttribute("data-root") || "";
  var form = document.getElementById("version-form");
  var select = document.getElementById("version-picker");
  if (!form || !select || typeof VERSIONS === "undefined") return;

  function find(id) {
    for (var i = 0; i < VERSIONS.length; i++) if (VERSIONS[i].id === id) return VERSIONS[i];
    return null;
  }

  // Newest first, dev builds under the release they lead to.
  select.textContent = "";
  var groups = {};
  for (var i = VERSIONS.length - 1; i >= 0; i--) {
    var v = VERSIONS[i];
    var group = groups[v.release];
    if (!group) {
      group = groups[v.release] = document.createElement("optgroup");
      group.label = v.release;
      select.appendChild(group);
    }
    var option = document.createElement("option");
    option.value = v.id;
    option.textContent = v.id + (v.id === LATEST ? " (latest)" : v.kind === "dev" ? " (dev)" : "");
    if (v.id === current) option.selected = true;
    group.appendChild(option);
  }

  // The same page and anchor in the chosen version when it has them,
  // otherwise its index, which says what was missing.
  function go(id) {
    var v = find(id);
    if (!v || id === current) return;
    var base = root + "../" + encodeURIComponent(id) + "/";
    var anchor = decodeURIComponent(location.hash.slice(1));
    if (anchor.indexOf("missing=") === 0) anchor = "";
    var anchors = v.pages[page];
    if (anchors && (!anchor || anchors.indexOf(anchor) >= 0)) {
      location.href = base + page + (anchor ? "#" + encodeURIComponent(anchor) : "");
    } else {
      location.href = base + "index.html#missing=" + encodeURIComponent(page + (anchor ? "#" + anchor : ""));
    }
  }

  // A pointer choice goes at once; from the keyboard, arrows only move the
  // choice and Enter (or the Go button) goes, so browsing doesn't navigate.
  var keyed = false;
  select.addEventListener("keydown", function (e) {
    keyed = true;
    if (e.key === "Enter") {
      e.preventDefault();
      go(select.value);
    }
  });
  select.addEventListener("pointerdown", function () { keyed = false; });
  select.addEventListener("change", function () { if (!keyed) go(select.value); });
  form.addEventListener("submit", function (e) {
    e.preventDefault();
    go(select.value);
  });

  // Arrived from another version's page that this one doesn't have.
  var note = document.getElementById("version-note");
  if (note && location.hash.indexOf("#missing=") === 0) {
    var missing = decodeURIComponent(location.hash.slice("#missing=".length));
    note.textContent = "Version " + current + " has no " + missing + ", so this is its overview.";
    note.hidden = false;
  }
})();
