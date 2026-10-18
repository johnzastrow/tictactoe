"""What each recording does: one list of steps per control, shared by every client.

gen_tapes.py turns each list into a VHS tape for the two terminal clients;
capture_web.py replays the same list with Playwright in the two web clients.
So all four clients get exactly the same keys at the same pace.

Every item starts with the sidebar focused on its tab (the terminal clients
press Down tab-1 times from startup; the web clients click the tab in the
sidebar). Steps:

    ("k", KEY, n)   press KEY n times. KEY is a VHS key name: Up Down Left Right
                    Enter Tab Escape Space Backspace PageUp PageDown, or Ctrl+X
    ("t", TEXT)     type TEXT at the tape's typing speed
    ("s", MS)       wait
    "open"          the control opens here (clips are lined up on this moment)
    "shot"          take the still (after a short settle)
    "reset"         Escape back to the sidebar, then Ctrl+R and y: put the
                    tab back to its defaults so later items start clean

VHS can't send Home/End, Shift+Tab, Shift+arrows or F-keys, so none are used.
"""

TYPE_MS = 120  # Set TypingSpeed in the tapes, and Playwright's typing delay

# The 16 tabs in sidebar order: (id, title), as in docs/demo/ui-spec.md §4
TABS = [("text", "Text"), ("number", "Number"), ("date", "Date"), ("select", "Dropdown"),
        ("combo", "Combobox"), ("tags", "Tags"), ("textarea", "Text area"),
        ("choice", "Choices"), ("cascade", "Cascade"), ("grid", "Grid"), ("menus", "Menus"),
        ("palette", "Palette"), ("dialogs", "Dialogs"), ("charts", "Charts"),
        ("kanban", "Kanban"), ("gantt", "Gantt")]

# (name, tab number, title, steps)
ITEMS = [
    ("01-text", 1, "Text input with live validation", [
        "open", ("k", "Enter", 1), ("s", 300), ("k", "Tab", 2), ("s", 300),
        ("k", "Ctrl+U", 1), ("t", "ab-12"), "shot", ("k", "Escape", 2)]),
    ("02-number", 2, "Number spinner and slider", [
        "open", ("k", "Enter", 1), ("s", 300), ("k", "Up", 3), ("s", 300),
        ("k", "Tab", 2), ("s", 300), ("k", "Right", 6), "shot", "reset"]),
    ("03-date", 3, "Natural-language date with calendar", [
        "open", ("k", "Enter", 1), ("s", 300), ("k", "Ctrl+U", 1), ("t", "fri"),
        "shot", ("k", "Escape", 2)]),
    ("04-select", 4, "Dropdown select", [
        ("k", "Enter", 1), ("s", 300), "open", ("k", "Enter", 1), ("s", 300),
        ("k", "Down", 2), "shot", ("k", "Escape", 2)]),
    ("05-combo", 5, "Fuzzy combobox", [
        ("k", "Enter", 1), ("s", 300), "open", ("t", "gl"), "shot", ("k", "Escape", 2)]),
    ("06-tags", 6, "Tag chips with suggestions", [
        ("k", "Enter", 1), ("s", 300), "open", ("t", "se"), "shot", ("k", "Escape", 2)]),
    ("07-textarea", 7, "Multi-line text area", [
        ("k", "Enter", 1), ("s", 300), "open",
        ("t", "Typed live, saved with Ctrl+S."), ("k", "Enter", 1), "shot", ("k", "Escape", 1), ("s", 400),
        ("t", "y"), ("k", "Escape", 1)]),
    ("08-choice", 8, "Checkboxes, radio buttons, toggle", [
        ("k", "Enter", 1), ("s", 300), "open", ("k", "Tab", 1), ("k", "Right", 1),
        ("k", "Space", 1), ("s", 300), ("k", "Tab", 1), ("k", "Right", 1), ("k", "Space", 1),
        ("s", 300), ("k", "Tab", 1), ("k", "Space", 1), "shot", "reset"]),
    ("09-cascade", 9, "Cascading selects", [
        ("k", "Enter", 1), ("s", 300), "open", ("k", "Enter", 1), ("s", 300),
        ("k", "Down", 1), ("k", "Enter", 1), ("s", 400), ("k", "Tab", 1), ("k", "Enter", 1),
        "shot", ("k", "Escape", 1), "reset"]),
    ("10-grid", 10, "Editable data grid", [
        ("k", "Enter", 1), ("s", 300), ("k", "Down", 2), ("k", "Right", 4), ("s", 300),
        "open", ("k", "Enter", 1), ("s", 300), ("k", "Down", 1), "shot", ("k", "Escape", 3)]),
    ("11-menus", 11, "Menu bar with submenus", [
        ("k", "Enter", 1), ("s", 300), "open", ("k", "Right", 1), ("k", "Enter", 1),
        ("s", 300), ("k", "Enter", 1), "shot", ("k", "Escape", 4)]),
    ("12-context-menu", 11, "Context menu", [
        ("k", "Enter", 1), ("s", 300), ("k", "Tab", 1), ("k", "Down", 1), ("s", 300),
        "open", ("k", "Enter", 1), "shot", ("k", "Escape", 3)]),
    ("13-palette", 12, "Command palette", [
        ("k", "Enter", 1), ("s", 300), "open", ("t", "gan"), "shot", ("k", "Escape", 2)]),
    ("14-dialog", 13, "Modal form dialog", [
        ("k", "Enter", 1), ("s", 300), ("k", "Right", 1), "open", ("k", "Enter", 1),
        ("s", 300), ("k", "Tab", 1), ("k", "Ctrl+U", 1), ("t", "not-an-email"), "shot",
        ("k", "Escape", 3)]),
    ("15-charts", 14, "Bar chart, sparkline, gauge, heat strip", [
        ("k", "Enter", 1), ("s", 300), "open", ("k", "Right", 3), ("s", 300),
        ("k", "Up", 4), "shot", "reset"]),
    ("16-kanban", 15, "Kanban board", [
        ("k", "Enter", 1), ("s", 300), ("k", "Right", 1), ("s", 300), "open",
        ("t", ">"), ("s", 500), ("t", ">"), "shot", "reset"]),
    ("17-gantt", 16, "Gantt chart", [
        ("k", "Enter", 1), ("s", 300), ("k", "Down", 1), ("s", 300), "open",
        ("t", ">>>"), ("s", 300), ("t", "]]"), "shot", "reset"]),
    ("18-help", 3, "Searchable help", [
        "open", ("k", "Ctrl+G", 1), ("s", 600), ("t", "save"), "shot", ("k", "Escape", 1)]),
]

TITLES = {name: title for name, _, title, _ in ITEMS}
