"""Writes the 3-version fixture from ../model.json (python3 make.py).

0.1.0 (a release built before headers were frozen, so it has none), then
0.2.0-dev.1 (a dev build), 0.2.0 (the release it leads to) and the
unreleased working tree. Each carries the changelog from the one before it,
as core computes it; nothing here judges a change. The release 0.2.0 also
carries its changes since 0.1.0 (span `release`): its dev build's and its
own.
"""
import copy, json, pathlib

here = pathlib.Path(__file__).parent
base = json.loads((here / "../model.json").read_text())

def unit(model, name):
    return next(u for m in model["modules"] for u in m["units"] if u.get("name") == name)

def log(frm, to, breaking=(), features=(), modifications=(), span=None):
    out = {"from": frm, "to": to, "breaking": list(breaking), "features": list(features), "modifications": list(modifications)}
    if span:
        out["span"] = span
    return out

v1 = copy.deepcopy(base)
v1["package"]["version"] = "0.1.0"
v1["package"]["docs"] = None
for m in v1["modules"]:
    m["docs"] = None
    for u in m["units"]:
        u.pop("wire", None)
unit(v1, "Customer")["fields"] = [f for f in unit(v1, "Customer")["fields"] if f["name"] != "phone"]
unit(v1, "Payment")["variants"].append("Cash")
v1["changelog"] = []

dev = copy.deepcopy(base)
dev["package"]["version"] = "0.2.0-dev.1"
for m in dev["modules"]:
    for u in m["units"]:
        u.pop("wire", None)
dev["changelog"] = [log("0.1.0", "0.2.0-dev.1",
    breaking=[b for b in base["changelog"][0]["breaking"] if b["kind"] == "removed_enum_variant"],
    features=base["changelog"][0]["features"])]

rel = copy.deepcopy(dev)
rel["package"]["version"] = "0.2.0"
docstring = [{"kind": "docstring_changed", "name": "Order"}]
# The step from the dev build (what a comparison composes), and the
# release's changes since the previous release, which its page shows.
rel["changelog"] = [log("0.2.0-dev.1", "0.2.0", modifications=docstring),
    log("0.1.0", "0.2.0", breaking=dev["changelog"][0]["breaking"],
        features=dev["changelog"][0]["features"], modifications=docstring, span="release")]

now = copy.deepcopy(base)
now["package"]["version"] = None
orders = unit(now, "Orders")
cancel = copy.deepcopy(orders["functions"][0])
cancel.update(name="cancel", docs={"text": "Cancel an order.", "params": []}, returns={"kind": "primitive", "name": "bool"}, throws=[])
orders["functions"].append(cancel)
now["changelog"] = [log("0.2.0", "working tree", features=[
    {"kind": "added_function", "protocol_name": "Orders", "function_name": "cancel",
     "signature": "(order: Order) -> bool", "new": "function cancel(order: Order) -> bool;"}])]

for name, model in [("0.1.0", v1), ("0.2.0-dev.1", dev), ("0.2.0", rel), ("unreleased", now)]:
    (here / f"{name}.json").write_text(json.dumps(model, indent=2) + "\n")
