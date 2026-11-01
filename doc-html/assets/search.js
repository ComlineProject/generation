/* comline-doc-html: client-side search over the index above. No network:
   the data is inlined, so it works from file:// too. */
(function () {
  var script = document.currentScript;
  var root = (script && script.getAttribute("data-root")) || "";
  var input = document.getElementById("search");
  var list = document.getElementById("results");
  if (!input || !list) return;
  var entries = INDEX.map(function (e) {
    return { e: e, name: e.n.toLowerCase(), hay: (e.n + " " + e.k + " " + e.m + " " + e.s).toLowerCase() };
  });
  function score(x, q, terms) {
    for (var i = 0; i < terms.length; i++) if (x.hay.indexOf(terms[i]) < 0) return -1;
    if (x.name === q) return 0;
    if (x.name.indexOf(q) === 0) return 1;
    if (x.name.indexOf(q) >= 0) return 2;
    return 3;
  }
  function item(e) {
    var li = document.createElement("li");
    var a = document.createElement("a");
    a.href = root + e.u;
    var kind = document.createElement("span");
    kind.className = "kind";
    kind.textContent = e.k + " ";
    var name = document.createElement("code");
    name.textContent = e.n;
    var where = document.createElement("small");
    where.textContent = e.m + (e.s ? " — " + e.s : "");
    a.appendChild(kind);
    a.appendChild(name);
    a.appendChild(where);
    li.appendChild(a);
    return li;
  }
  function run() {
    var q = input.value.trim().toLowerCase();
    list.textContent = "";
    if (!q) { list.hidden = true; return; }
    var terms = q.split(/\s+/);
    var hits = [];
    entries.forEach(function (x, i) {
      var s = score(x, q, terms);
      if (s >= 0) hits.push([s, i]);
    });
    hits.sort(function (a, b) { return a[0] - b[0] || a[1] - b[1]; });
    hits.slice(0, 30).forEach(function (h) { list.appendChild(item(entries[h[1]].e)); });
    if (!hits.length) {
      var li = document.createElement("li");
      li.className = "none";
      li.textContent = "No results";
      list.appendChild(li);
    }
    list.hidden = false;
  }
  input.addEventListener("input", run);
  input.addEventListener("keydown", function (ev) {
    if (ev.key === "Enter") {
      var first = list.querySelector("a");
      if (first) location.href = first.href;
    } else if (ev.key === "Escape") {
      input.value = "";
      run();
    } else if (ev.key === "ArrowDown") {
      var a = list.querySelector("a");
      if (a) { a.focus(); ev.preventDefault(); }
    }
  });
  list.addEventListener("keydown", function (ev) {
    var li = document.activeElement && document.activeElement.parentNode;
    if (ev.key === "ArrowDown" && li && li.nextSibling) { li.nextSibling.firstChild.focus(); ev.preventDefault(); }
    if (ev.key === "ArrowUp" && li) {
      if (li.previousSibling) li.previousSibling.firstChild.focus(); else input.focus();
      ev.preventDefault();
    }
    if (ev.key === "Escape") input.focus();
  });
  document.addEventListener("keydown", function (ev) {
    if (ev.key === "/" && document.activeElement !== input) { input.focus(); ev.preventDefault(); }
  });
  document.addEventListener("click", function (ev) {
    if (!list.contains(ev.target) && ev.target !== input) list.hidden = true;
  });
  input.addEventListener("focus", function () { if (input.value.trim()) run(); });
})();
