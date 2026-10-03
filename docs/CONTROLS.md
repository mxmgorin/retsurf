# Controls

`bindings.toml` maps gamepad and keyboard gestures to browser actions; input maps in
`input_maps/` decide what reaches a web game in Game Mode. Both live in the data dir next to
`config.toml` and are editable in the settings overlay (Controls and Gaming tabs).

## Bindings (`bindings.toml`)

```toml
[gamepad]
a = "confirm"              # tap: fires on press
"hold:r1" = "reload"       # hold for [controls] hold_ms
"l2+r2" = "zoom_reset"     # chord: press one while holding the other
y = "none"                 # unbind

[keyboard]
"ctrl+r" = "reload"        # shortcuts with Ctrl or Alt always fire
f = "hints"                # plain keys fire only while no text field has focus
k = "nav_up"
```

**Gamepad buttons**: `a b x y l1 r1 l2 r2 l3 r3 start select`. The D-pad aims the cursor and
is not bindable. While the on-screen keyboard is open, L2/R2 act as Shift and Enter. A button
with a hold or chord fires its tap on release, and `confirm` takes no hold or chord, since
clicks and drags need its press.

**Keyboard**: any key with optional `ctrl`, `alt`, `shift`, matched exactly. Every default
is on Ctrl, so pages get all plain keys:

| Keys | Action | Keys | Action |
| --- | --- | --- | --- |
| `ctrl+r` | reload | `ctrl+left` / `ctrl+right` | back / forward |
| `ctrl+b` | bookmark | `ctrl+t` / `ctrl+w` | new / close tab |
| `ctrl+h` | home | `ctrl+tab` / `ctrl+shift+tab` | next / previous tab |
| `ctrl+e` | reader view | `ctrl+=` / `ctrl+-` / `ctrl+0` | zoom in / out / reset |
| `ctrl+m` | Quick Menu | `ctrl+f` | link hints |
| `ctrl+l` | address bar | `ctrl+,` | settings |
| `ctrl+alt+g` | Quick Access | arrows | overlay navigation |

**Actions**: `confirm`, `cancel`, `menu` (Quick Menu), `quick_access` (Quick Access, the way
in and out of Game Mode), `settings`, `osk` (on-screen keyboard), `quit`, `prev` / `next`
(menu section or history), `home`, `address`, `hints`, `scroll` (toggle the stick between
cursor and scrolling; unbound by default), `nav_up` / `nav_down` / `nav_left` / `nav_right`
(a step in the open overlay, else the key goes to the page), `reload`, `reader`,
`bookmark`, `zoom_in` / `zoom_out` / `zoom_reset`, `tab_next` / `tab_prev` / `new_tab` /
`close_tab`, `none`.

Invalid entries are logged and skipped. An action with nothing bound gets its defaults back
at startup, so actions added in a later release reach old files; to disable an action, bind
it to a gesture you never press.

## Game Mode input maps (`input_maps/*.toml`)

An input map is what each button, stick and key sends to the page in Game Mode.
`[game_mode] input_map` picks one by id. Built in:

| id | name | the game gets |
| --- | --- | --- |
| `keys` | Keyboard (arrows and Z/X) | the PICO-8 and js13k convention; most of itch.io works as is |
| `wasd` | Keyboard (WASD) | WASD on the left stick, Space / E / R / F / Shift / Control around it |
| `mouse` | Mouse only | left stick moves the cursor, A clicks, right stick scrolls |

`none` maps nothing: the game reads the pad and keys itself, with no cursor. There is no
first-person map: Servo has no Pointer Lock.

`input_maps/<id>.toml` in the data dir adds a map, or replaces the built-in of that id until
the file is deleted. Files are read at startup; a bad entry is logged and skipped.

### In the app

The **Input map** row (Quick Access in Game Mode, or Settings > Gaming) lists the maps.
**+ Add** creates an empty one (everything passes through), and **A** on a map offers *Use
this map*, *Edit*, *Rename*, *Duplicate*, *Delete*, or *Reset to default* for an edited
built-in.

The editor has a row per bound source, grouped as *Sticks*, *Gamepad* and *Keyboard*. **A**
picks what the source sends, **X** unbinds it, **B** saves. **+ Add** listens for the
button, key or stick to bind; it refuses holds, chords and modified keys, and gives up after
six seconds. Editing a built-in writes the file that replaces it.

### File format

```toml
name = "Vampire Survivors"    # shown in the menu; the file name is the id

[pad]                         # buttons and D-pad, by the bindings.toml names
up = "key.ArrowUp"
a = "key.Space"
x = { to = "key.x", code = "KeyY", shift = true }   # when key and code differ
y = "key.Shift"
r2 = "mouse.left"             # click at the cursor; also .right and .middle
l3 = "mouse.scroll.down"      # scroll while held
down = "mouse.cursor.down"    # move the cursor while held
select = "pad.start"          # a button of the page's Gamepad API
l2 = "passthrough"            # reaches the page as itself
r1 = "none"                   # inert
l1 = "layer:aim"              # holds the layer open

[stick.left]                  # four directions, past [controls] deadzone
up = "key.ArrowUp"
down = "key.ArrowDown"
left = "key.ArrowLeft"
right = "key.ArrowRight"

[stick.right]
analog = "mouse.cursor"       # or mouse.scroll, or none

[key]                         # physical keys; unlisted ones pass through
w = "key.ArrowUp"

[layer.aim.pad]               # while l1 is held
a = "key.Shift"
[layer.aim.key]
w = "key.ArrowDown"
```

**Targets**: `key.<name>` (one character, or a standard name like `ArrowUp`, `Enter`,
`Escape`, `Shift`), `pad.<button>`, `mouse.left` / `.right` / `.middle`,
`mouse.scroll.<dir>`, `mouse.cursor.<dir>`, `mouse.cursor` / `mouse.scroll` for a whole
stick, `passthrough`, `none`, `layer:<name>`. The table form adds `code`, `shift` / `ctrl`
/ `alt` and `speed`. A `pad.<button>` from the keyboard goes to an extra pad, "retsurf
mapped pad", announced while such a map runs.

**Rules**:

- A bound source is withheld from the page; only `passthrough` reaches it as itself. A stick
  read as directions withholds its whole axis.
- The button of the `quick_access` gesture: a bare tap opens the menu and never reaches the
  game. Under a hold or chord it is the map's, sent on release.
- Layers apply while held, carry buttons and keys but not sticks, and do not nest. A key
  stays held until its source is released, even if the layer closes first.
