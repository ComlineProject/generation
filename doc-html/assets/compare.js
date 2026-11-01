/* comline-doc-html: comparing versions, over COMPARE above. No network:
   the data is inlined, so it works from file:// too.

   The change labels are core's: each version's changes are its changelog
   from the version before it, already split into major, minor and patch.
   Comparing A with B only collects the steps after A up to B; nothing here
   decides whether a change is breaking. */
(function () {
  var choices = document.getElementById("choices");
  var out = document.getElementById("comparison");
  if (!choices || !out || typeof COMPARE === "undefined") return;
  var RANK = { major: 0, minor: 1, patch: 2 };

  function el(tag, text, cls) {
    var e = document.createElement(tag);
    if (text != null) e.textContent = text;
    if (cls) e.className = cls;
    return e;
  }
  function badge(severity) {
    return el("span", severity, "badge " + severity);
  }
  function worst(changes) {
    var best = null;
    for (var i = 0; i < changes.length; i++) {
      if (best === null || RANK[changes[i].severity] < RANK[best]) best = changes[i].severity;
    }
    return best;
  }

  // Newest first, each a checkbox; the two newest start checked.
  var boxes = [];
  for (var i = COMPARE.length - 1; i >= 0; i--) {
    var v = COMPARE[i];
    var label = el("label", null, "choice");
    var box = document.createElement("input");
    box.type = "checkbox";
    box.value = String(i);
    box.checked = i >= COMPARE.length - 2;
    box.addEventListener("change", show);
    label.appendChild(box);
    label.appendChild(document.createTextNode(" " + v.id + (v.kind === "dev" ? " (dev)" : "")));
    choices.appendChild(label);
    boxes.push(box);
  }

  // The steps from version a (exclusive) to b (inclusive), each change
  // tagged with the step it came from.
  function steps(a, b) {
    var all = [];
    for (var i = a + 1; i <= b; i++) {
      for (var j = 0; j < COMPARE[i].changes.length; j++) {
        var c = COMPARE[i].changes[j];
        all.push({ unit: c.unit, severity: c.severity, title: c.title, html: c.html, step: COMPARE[i - 1].id + " to " + COMPARE[i].id });
      }
    }
    return all;
  }

  function diff(a, b) {
    var changes = steps(a, b);
    var head = el("h2", COMPARE[a].id + " to " + COMPARE[b].id + " ");
    var overall = worst(changes);
    if (overall) head.appendChild(badge(overall));
    out.appendChild(head);
    if (!changes.length) {
      out.appendChild(el("p", "No changes."));
      return;
    }
    var units = [], byUnit = {};
    changes.forEach(function (c) {
      var key = c.unit || "(package)";
      if (!byUnit[key]) { byUnit[key] = []; units.push(key); }
      byUnit[key].push(c);
    });
    units.sort();
    units.forEach(function (unit) {
      var h = el("h3", null);
      h.appendChild(el("code", unit));
      h.appendChild(document.createTextNode(" "));
      h.appendChild(badge(worst(byUnit[unit])));
      out.appendChild(h);
      var ul = el("ul", null, "changes entries");
      byUnit[unit].forEach(function (c) {
        // The entry is rendered by comline-doc-html, already escaped.
        var li = el("li", null);
        li.innerHTML = c.html;
        li.appendChild(el("small", "(" + c.step + ")"));
        ul.appendChild(li);
      });
      out.appendChild(ul);
    });
  }

  function grid(chosen) {
    var first = chosen[0], last = chosen[chosen.length - 1];
    var bySeverity = {}, titles = {};
    steps(first, last).forEach(function (c) {
      var s = bySeverity[c.unit];
      if (!s || RANK[c.severity] < RANK[s]) bySeverity[c.unit] = c.severity;
      (titles[c.unit] = titles[c.unit] || []).push(c.severity + ": " + c.title + " (" + c.step + ")");
    });
    var keys = {}, list = [];
    chosen.forEach(function (i) {
      for (var k in COMPARE[i].rows) if (!keys[k]) { keys[k] = true; list.push(k); }
    });
    list.sort();
    out.appendChild(el("h2", COMPARE[first].id + " to " + COMPARE[last].id));
    var wrap = el("div", null, "table");
    var table = el("table", null, "grid");
    var tr = el("tr", null);
    tr.appendChild(el("th", "Unit or member"));
    tr.appendChild(el("th", "Change"));
    chosen.forEach(function (i) { tr.appendChild(el("th", COMPARE[i].id)); });
    var thead = el("thead", null);
    thead.appendChild(tr);
    table.appendChild(thead);
    var tbody = el("tbody", null);
    list.forEach(function (key) {
      var row = el("tr", null, key.indexOf(".") < 0 ? "unit-row" : "member-row");
      var name = el("td", null);
      name.appendChild(el("code", key));
      row.appendChild(name);
      var unit = key.split("::").pop().split(".")[0];
      var sev = el("td", null);
      if (key.indexOf(".") < 0 && bySeverity[unit]) {
        sev.appendChild(badge(bySeverity[unit]));
        sev.title = titles[unit].join("\n");
      }
      row.appendChild(sev);
      var previous;
      chosen.forEach(function (i, n) {
        var value = COMPARE[i].rows[key];
        var td = el("td", null);
        if (value == null) {
          td.appendChild(el("span", "absent", "absent"));
        } else {
          td.appendChild(el("code", value));
        }
        if (n > 0 && value !== previous) td.className = "moved";
        previous = value;
        row.appendChild(td);
      });
      tbody.appendChild(row);
    });
    table.appendChild(tbody);
    wrap.appendChild(table);
    out.appendChild(wrap);
  }

  function show() {
    out.textContent = "";
    var chosen = [];
    boxes.forEach(function (b) { if (b.checked) chosen.push(Number(b.value)); });
    chosen.sort(function (x, y) { return x - y; });
    if (chosen.length < 2) {
      out.appendChild(el("p", "Choose two versions or more."));
    } else if (chosen.length === 2) {
      diff(chosen[0], chosen[1]);
    } else {
      grid(chosen);
    }
  }
  show();
})();
