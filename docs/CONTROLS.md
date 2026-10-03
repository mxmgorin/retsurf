# Controls

What the gamepad and keyboard do: `bindings.toml` maps gestures to browser actions, and
the input maps in `input_maps/` decide what reaches a web game while Game Mode is on. Both
are also editable in-app, in the settings overlay's Controls and Gaming tabs.

## Bindings (`bindings.toml`)

Gamepad and keyboard layouts live in `bindings.toml`, next to `config.toml` (a
template with the defaults is written on first run). Each
entry maps a *gesture* to an *action*:

```toml
[gamepad]
a = "confirm"              # tap: fires on press
"hold:r1" = "reload"       # hold the button for hold_ms
"l2+r2" = "zoom_reset"     # chord: press one while holding the other
y = "none"                 # explicitly unbind

[keyboard]
"ctrl+r" = "reload"        # modifier shortcuts always fire
f = "hints"                # plain keys fire only while no text input has focus
k = "nav_up"               # overlay navigation can move to vim-style keys
```

**Gamepad gestures**: a tap (`a`), a hold (`"hold:a"`), or a button chord
(`"a+b"`). Buttons: `a b x y l1 r1 l2 r2 l3 r3 start select` (the D-pad aims
the cursor and is not bindable; L2/R2 double as Shift and Enter (on the wheel,
Shift and the held digits layer), but only while the on-screen keyboard is open). A
button with a hold or chord gesture fires its tap on release instead of press
(the gesture is ambiguous until then); `confirm` needs the press edge for
clicks and drags, so hold/chord gestures on its button are rejected.

**Keyboard shortcuts**: any key with optional `ctrl`/`alt`/`shift` modifiers,
matched strictly. Plain keys (no Ctrl/Alt) are muted whenever a text input —
on the page or the address bar — holds focus, so they can't hijack typing.
Every default is on Ctrl, so a page (a game) gets all plain keys:
`ctrl+r` reload · `ctrl+b` bookmark · `ctrl+h` home · `ctrl+e` reader view ·
`ctrl+m` menu · `ctrl+l` address · `ctrl+,` settings · `ctrl+f` link hints ·
`ctrl+alt+g` Quick Access · `ctrl+left`/`ctrl+right` back/forward ·
`ctrl+t`/`ctrl+w` new/close tab · `ctrl+tab`/`ctrl+shift+tab` next/previous
tab · `ctrl+=`/`ctrl+-`/`ctrl+0` zoom in/out/reset. The arrows are bound too,
but only as overlay navigation: with no overlay open they go to the page.

**Actions**: `confirm` (click/select) · `cancel` (close/back) · `osk`
(on-screen keyboard) · `reload` · `prev` / `next` (menu section or history) ·
`hints` (link hints) · `bookmark` · `reader` (reader mode) · `menu` ·
`settings` (settings overlay; pressed again while it's open, closes it) · `home`
(go to the home page) · `address` (type a new address) · `quick_access` (Quick
Access, the way in and out of Game Mode) · `quit` (quit the app) · `tab_next` /
`tab_prev` /
`new_tab` / `close_tab` ·
`zoom_in` / `zoom_out` / `zoom_reset` (page zoom along a Firefox-style 50–300%
ladder / back to the config default) ·
`nav_up` / `nav_down` / `nav_left` / `nav_right` (one step in whatever overlay
is open — menu, on-screen keyboard, or link hints; with none open the key goes
to the page) · `scroll` (gamepad-only: toggle the D-pad / left stick between
cursor and page scroll; unbound by default, since pushing the cursor against
an edge scrolls too) · `none`.

Invalid buttons, keys, actions, or gestures are logged and skipped at startup —
check the log if a binding doesn't respond.

**Upgrades.** The file is written only when it is missing, so an action added in a
later release would be unreachable in a file written before it. At startup any
action with *nothing* bound on a device gets its default gestures back there (one
log line each); a gesture the file already spells is never taken back, and an
action you rebound is not missing, so your layout stands. The one consequence:
clearing an action's last gesture doesn't stick — to make an action inert, bind
it to a gesture you never press rather than removing it.

## Game Mode input maps (`input_maps/*.toml`)

An input map is what each button, stick direction and key sends to the page while
Game Mode is on. Three ship built in, named for what the game sees rather than
for what the pad becomes; `[game_mode] input_map` picks one by id, or `none`
for no map at all, which a game that reads the Gamepad API or the keys itself
wants: nothing is remapped, and there is no cursor and no click. `none` is not a
file and cannot be edited; an `input_maps/none.toml` is ignored.

| id | name | what the game gets |
| --- | --- | --- |
| `keys` | Keyboard (arrows and Z/X) | the retro convention PICO-8 exports and js13k entries share — most of itch.io plays with no edit at all |
| `wasd` | Keyboard (WASD) | WASD on the left stick, with Space / E / R / F / Shift / Control round it |
| `mouse` | Mouse only | the left stick moves the cursor, A presses, the right stick scrolls |

There is no first-person template: Servo has no Pointer Lock, so a stick cannot
turn a camera, and only the left mouse button has a route.

The built-ins live in the binary and are always offered, so a later release can
add one without touching your files. Put an `input_maps/<id>.toml` in the data dir
to add a map of your own, or name it after a built-in to replace that one —
deleting the file restores it. A file is read at startup; a typo costs its own
binding and is logged, not the whole map.

**The Game Mode menu's "Input map" row** is all of this without a keyboard or a
file manager, which is the only way to do it on a handheld. It opens the list of
maps, marked with the one in use and led by **+ Add**; **A** on any map
opens its own screen:

| row | what it does |
| --- | --- |
| Use this map | hands it to Game Mode and writes `[game_mode] input_map` |
| Edit | the editor below |
| Rename... | the on-screen keyboard types a new name; the file's stem stays as it is |
| Duplicate... | a copy under a name you type, bindings and all |
| Delete | throws the file away, after a confirmation |
| Reset to default | the same, on a built-in: the binary's own version comes back |

**+ Add**, the row above them, types a name and adds a map that binds nothing, which is
passthrough: the whole pad reaches the page raw, with no cursor and no click
until the editor gives it one. It opens on the new map, since that is what it
was made for. A built-in the binary carries and no file shadows has nothing to
remove, so it offers neither of the last two.

**The editor** is a row per source the map binds, under a heading per device —
*Sticks*, *Gamepad*, *Keyboard*, which is what names a row's table, so the row
itself is just `a`, `q` or `left.up`. A stick is a source like the rest: a row
once it is bound, and none before. **A** opens what that source can send, **X**
unbinds it
(its row goes, and the source reaches the page as itself again), **B** saves. For
a button or a key that is *Keyboard*, *Mouse*, *Gamepad*, *Passthrough* or
*Ignore*. *Mouse* and *Gamepad* open their own lists — the three buttons and the
four cursor and four scroll steps (or *Cursor* and *Scroll* over a stick), and
the pad's sixteen buttons — and *Keyboard* hands over to the on-screen keyboard, dimmed behind so
it reads as a question rather than a keyboard. Its **Fn** key swaps to the keys no
character grid carries — Escape, F1-F12, the navigation cluster, and Shift /
Control / Alt / Meta on their own, which is what a game wanting a run or crouch
key binds.

**+ Add**, the editor's last row, listens: press the button or key you want to
map, or push the stick, and its list opens straight away, so a keyboard key is
bound the way a pad button is and a pad this build has never heard of needs no
table of its own. A stick has no gesture of its own, so it is taken from a push
most of the way over — past any dead zone, since one resting off-centre must not
bind itself. A map holds one target per source, so
a hold, a chord or a modified key is refused on the row that asked; so is the
button the `quick_access` gesture resolves on the press, where it is a bare tap.
Nothing pressed within six seconds gives up on its own — a handheld has no Esc.

**A stick answers A with its own list**, like every other row — *Cursor*,
*Scroll*, *Passthrough*, *Ignore*, or *Four directions*, which seeds the arrows
and puts a `stick.<side>.<direction>` row under it for each, edited like a
button. The file is one form or the other, so picking either takes the other
away. A direction is offered no Passthrough: a stick read as directions withholds
the whole axis, so the page would see nothing either way.

Editing a built-in writes the `input_maps/<id>.toml` that replaces it, so deleting
that file is still how you get the original back. The layers below are the file's:
they need names the screen has no room to pick.

```toml
name = "Vampire Survivors"    # what the menu shows; the file name is the id

[pad]                         # buttons and the D-pad, by the bindings.toml names
up = "key.ArrowUp"
a = "key.Space"
b = "key.z"
x = { to = "key.x", code = "KeyY", shift = true }   # when key and code differ
y = "key.Shift"               # a bare modifier: takes the left-hand `code`
r2 = "mouse.left"             # a mouse button at the cursor; also .right/.middle
l3 = "mouse.scroll.down"      # scrolls a step per frame while it is held
down = "mouse.cursor.down"    # moves the cursor a step per frame while it is held
select = "pad.start"          # a button of the page's own Gamepad API
l2 = "passthrough"            # reaches the page as the gamepad button it is
r1 = "none"                   # consumed: inert while this map is active
l1 = "layer:aim"              # holds a layer open; sends nothing itself

[stick.left]                  # four directions, through [controls] deadzone
up = "key.ArrowUp"
down = "key.ArrowDown"
left = "key.ArrowLeft"
right = "key.ArrowRight"

[stick.right]
analog = "mouse.cursor"       # or mouse.scroll — the whole stick, not a direction

[key]                         # physical keys; unlisted ones reach the game as-is
w = "key.ArrowUp"

[layer.aim.pad]               # while l1 is held
a = "key.Shift"
[layer.aim.key]
w = "key.ArrowDown"
```

Sources and targets alike name the device they belong to, and the editor's rows
are named the same way: a row is `pad.a`, `key.w` or `stick.left.up`, which TOML
also reads as the line of the file it stands for.

**Targets** are `key.<name>`, `pad.<button>`, one of the mouse's, or
`passthrough`, `none`, `layer:<name>`. A `pad.<button>` target is the page's
Gamepad API rather than a key: it rides the pad the source came from, so a map
can deal a pad's own buttons out again, and a source that is no pad — a keyboard
key — gets one the browser announces for the purpose (`retsurf mapped pad`,
listed only while the mode runs a map that asks for it). The mouse's are `mouse.left` / `mouse.right` / `mouse.middle` (a
button at the cursor), `mouse.scroll.up` / `.down` / `.left` / `.right` and
`mouse.cursor.up` / `.down` / `.left` / `.right` (a held source scrolling the
page or moving the cursor a step per frame, at a fully-deflected stick's rate),
and `mouse.cursor` / `mouse.scroll` for a whole stick — the directional pair is
what points on a device whose sticks are the game's, or that has none. A key's name
is one character (`key.z`), `key.Space`, or a standard spelling (`key.ArrowUp`,
`key.Enter`, `key.Escape`, `key.Shift`); the `code` games branch on is derived
from it, and the table form `{ to = …, code = …, shift/ctrl/alt = true,
speed = 1.5 }` says it out loud where they differ. `speed` scales every one of
these, the steps included. A target that names no device is refused, so a misspelled
`passthrough` cannot quietly become a key.

**A bound source is withheld from the page's raw input**, so a button mapped to a
key is not also delivered as a gamepad button — only `passthrough` is. A stick
read as directions keeps its whole axis, since half an axis cannot be withheld —
which also makes `passthrough` on one direction meaningless. `analog = "none"`
keeps the axis and sends nothing, which is how a stick is made inert.

**The `quick_access` gesture's button is the map's to bind, but its press arrives
late.** A hold and a chord are undecided until the button is let go, so a map's
target for it is sent on release and ended a frame later, and an unbound one
reaches the page as the button it is. Only a bare tap resolves on the press
itself: that button opens the menu and never reaches the game, in any map or
layer, and the editor refuses a row for it.

**Layers** are held, not toggled: the activator sends nothing of its own, and a
button the layer leaves alone still means what `[pad]` says. What a source sends
is decided when it goes down, so releasing the activator never strands a key that
is still held. Layers carry buttons and keys, not sticks, and cannot open other
layers.
