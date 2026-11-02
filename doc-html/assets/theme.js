/* The theme toggle: auto (the system's setting), light, dark. `auto` sets no
   `data-theme` on <html>, so the stylesheet's prefers-color-scheme rule
   decides. The choice is kept in localStorage and in a token in window.name,
   which follows the tab from page to page even under file://, where
   browsers such as Firefox give every file its own localStorage. Internal
   links carry it as ?theme= so a page opened in a new tab has it too. The
   inline script in <head> applies it before the page paints. Every storage
   is tried, never trusted: any may be blocked. */
(function () {
  var KEY = "comline-doc-theme";
  var ORDER = ["auto", "light", "dark"];
  var ICON = { auto: "◐", light: "☀", dark: "☾" };
  var root = document.documentElement;
  var button = document.getElementById("theme-toggle");
  if (!button) return;
  var tokenPattern = new RegExp("(?:^|;)" + KEY + "=(auto|light|dark)(?=;|$)");
  // The choice made so far, or null while the reader hasn't made one.
  function saved() {
    try {
      var m = tokenPattern.exec(window.name);
      if (m) return m[1];
    } catch (e) {}
    return null;
  }
  function save(theme) {
    try {
      var rest = window.name.split(";").filter(function (p) {
        return p && p.indexOf(KEY + "=") !== 0;
      });
      rest.push(KEY + "=" + theme);
      window.name = rest.join(";");
    } catch (e) {}
    try {
      if (theme === "auto") localStorage.removeItem(KEY);
      else localStorage.setItem(KEY, theme);
    } catch (e) {}
  }
  function apply(theme) {
    if (theme === "auto") root.removeAttribute("data-theme");
    else root.setAttribute("data-theme", theme);
    var next = ORDER[(ORDER.indexOf(theme) + 1) % ORDER.length];
    var name = theme.charAt(0).toUpperCase() + theme.slice(1);
    button.textContent = ICON[theme] + " " + name;
    button.setAttribute("aria-label", "Theme: " + theme + ". Switch to " + next);
    button.title = "Theme: " + theme + " (switch to " + next + ")";
    button.setAttribute("data-theme-choice", theme);
  }
  button.addEventListener("click", function () {
    var now = button.getAttribute("data-theme-choice") || "auto";
    var next = ORDER[(ORDER.indexOf(now) + 1) % ORDER.length];
    save(next);
    apply(next);
  });
  // An internal link to another page carries the choice, so the page has it
  // even in a new tab (the window.name token only follows the same tab).
  function carry(event) {
    try {
      var a = event.target.closest && event.target.closest("a[href]");
      var choice = saved();
      if (!a) return;
      if (a.protocol !== location.protocol || a.host !== location.host) return;
      if (a.pathname === location.pathname) return;
      var rel = a.getAttribute("rel") || "";
      if (/\bexternal\b/.test(rel) || a.hasAttribute("download")) return;
      // Edit the written href, so a relative link stays relative.
      var raw = a.getAttribute("href");
      var hash = "";
      var at = raw.indexOf("#");
      if (at >= 0) { hash = raw.slice(at); raw = raw.slice(0, at); }
      var mark = raw.indexOf("?");
      var query = new URLSearchParams(mark >= 0 ? raw.slice(mark) : "");
      if (mark >= 0) raw = raw.slice(0, mark);
      if (choice) query.set("theme", choice);
      else query.delete("theme");
      var text = query.toString();
      a.setAttribute("href", raw + (text ? "?" + text : "") + hash);
    } catch (e) {}
  }
  document.addEventListener("click", carry, true);
  document.addEventListener("auxclick", carry, true);
  document.addEventListener("contextmenu", carry, true);
  apply(root.getAttribute("data-theme") || "auto");
  button.hidden = false;
})();
