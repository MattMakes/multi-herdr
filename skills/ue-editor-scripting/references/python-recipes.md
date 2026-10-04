# Editor Python recipes (UE 5.8)

Every name below is in Epic's Unreal Python 5.8 reference unless it is marked "unverified on 5.8". The 5.8 Python API is labeled Experimental: if a call is missing at run time, print `dir(unreal.<Class>)` in a read-only script and report what exists.

## Script skeleton

```python
import unreal

TASK = "inventory-defaults"
assets = unreal.get_editor_subsystem(unreal.EditorAssetSubsystem)
bel = unreal.BlueprintEditorLibrary
saved = []

def fail(msg):
    unreal.log_error(f"{TASK}: {msg}")
    raise RuntimeError(msg)

def load(path):
    if not assets.does_asset_exist(path):
        fail(f"missing asset {path}")
    obj = assets.load_asset(path)
    if obj is None:
        fail(f"cannot load {path}")
    return obj

# ... changes ...

for path in saved:
    if not assets.save_asset(path, only_if_is_dirty=True):
        fail(f"save failed {path}")
unreal.log(f"EDITOR-SCRIPT-OK {TASK} {len(saved)} assets saved")
```

## Blueprints

Create from a C++ parent:

```python
parent = unreal.load_class(None, "/Script/MyGame.MyPickup")   # or unreal.MyPickup.static_class()
bp = bel.create_blueprint_asset_with_parent("/Game/Pickups/BP_Coin", parent)
```

Set a class default (CDO) property:

```python
bp = load("/Game/Pickups/BP_Coin")
cdo = unreal.get_default_object(bel.generated_class(bp))
if cdo.get_editor_property("value") != 10:
    cdo.set_editor_property("value", 10)
    saved.append("/Game/Pickups/BP_Coin")
if not bel.compile_blueprint(bp):
    fail("compile failed BP_Coin")
```

Property names are the Python snake_case form of the C++ `UPROPERTY` name. `set_editor_property` fails on a property that is not editable in the editor; report it, and do not force it.

Variables and overrides:

```python
if "Score" not in [str(n) for n in bel.list_member_variable_names(bp)]:
    bel.add_member_variable(bp, "Score", bel.get_basic_type_by_name("int"))
    bel.set_blueprint_variable_instance_editable(bp, "Score", True)
event = bel.add_event_override(bp, "ReceiveBeginPlay", unreal.IntPoint(0, 0))   # position is an IntPoint on 5.8
graph = bel.add_function_override(bp, "CanPickUp")
```

Other calls: `add_function_graph`, `reparent_blueprint`, `replace_variable_references`, `change_member_variable_type`, `set_blueprint_variable_replication`, `add_event_dispatcher`, `remove_unused_variables`, `find_event_graph`, `find_graph`, `list_graph_names`, `list_functions`, `list_events`.

## Pins

Nodes come from what the library returns, for example the `K2Node_Event` from `add_event_override`. There is no documented 5.8 call to list every node in a graph or to spawn an arbitrary function-call node.

```python
then_pin = bel.find_then_pin(event)
in_pin = bel.find_input_pin(node, "Amount")
in_pin.set_pin_value("5")                       # returns False if the value is invalid
if not then_pin.try_create_connection(exec_pin):
    fail("cannot connect")
```

Also: `find_execute_pin`, `find_output_pin`, `find_result_pin`, `find_self_pin`, `list_all_pins`, `list_connected_pins`, `can_create_connection`, `break_single_pin_link`, `is_same_native_pin` (pin objects are proxies: compare with this, not `==`).

## Data tables

```python
table = load("/Game/Data/DT_Items")
csv = open(csv_path, encoding="utf-8").read()
if not unreal.DataTableFunctionLibrary.fill_data_table_from_csv_string(table, csv):
    fail("fill failed")
```

`fill_*` empties the table first. Keep the CSV or JSON source in the repository so the table can be rebuilt. Read back with `get_data_table_row_names` and `export_data_table_to_csv_string`.

## Levels and actors

```python
levels = unreal.get_editor_subsystem(unreal.LevelEditorSubsystem)
actors = unreal.get_editor_subsystem(unreal.EditorActorSubsystem)
if not levels.load_level("/Game/Maps/Arena"):
    fail("load level")
cls = bel.generated_class(load("/Game/Pickups/BP_Coin"))
a = actors.spawn_actor_from_class(cls, unreal.Vector(0, 0, 100))
a.set_actor_label("Coin_01")                     # unverified on 5.8: not on the 5.8 Actor page; skip if missing
if not levels.save_current_level():
    fail("save level")
```

A World Partition map stores actors in separate external files (one per actor). Expect new files under `__ExternalActors__`, and check them in step 6.

## Create, rename, duplicate

```python
tools = unreal.AssetToolsHelpers.get_asset_tools()
new = tools.create_asset("DA_Sword", "/Game/Data", unreal.MyItemData, None)   # factory None: unverified on 5.8 (typed Factory); pass a DataAssetFactory if it fails
tools.duplicate_asset("BP_Coin_Gold", "/Game/Pickups", load("/Game/Pickups/BP_Coin"))
tools.rename_assets([unreal.AssetRenameData(obj, "/Game/NewFolder", "NewName")])  # (asset, new_package_path, new_name) on 5.8
```

`rename_assets` fixes references and leaves redirectors. Fixing up redirectors afterwards is a separate project-wide change: ask first.

## Verification script

A second script, run in a new process, read-only:

```python
import unreal
assets = unreal.get_editor_subsystem(unreal.EditorAssetSubsystem)
bel = unreal.BlueprintEditorLibrary
fails = 0
def check(name, ok):
    global fails
    unreal.log(f"CHECK {'PASS' if ok else 'FAIL'} {name}")
    fails += 0 if ok else 1

bp = assets.load_asset("/Game/Pickups/BP_Coin")
cdo = unreal.get_default_object(bel.generated_class(bp))
check("BP_Coin value == 10", cdo.get_editor_property("value") == 10)
check("BP_Coin compiles", bel.compile_blueprint(bp))
if fails:
    unreal.log_error(f"VERIFY-FAIL {fails}")
    raise RuntimeError("verification failed")
unreal.log("VERIFY-OK inventory-defaults")
```

Do not save anything in the verification script. Report the count of `CHECK PASS` lines.
