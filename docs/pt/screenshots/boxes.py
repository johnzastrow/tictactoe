"""Crop boxes shared by compose.py and compose4.py."""

# TUI crop boxes (left, top, right, bottom) in terminal-frame pixels. The
# terminal is 1640x960 at 26 pt; cells are about 15.6 x 31 px.
BOXES = {
    "rust": {
        "01-combobox": (640, 100, 1540, 320),
        "02-dropdown": (1060, 100, 1625, 390),
        "03-date": (1080, 90, 1632, 520),
        "04-tags": (900, 105, 1632, 320),
        "05-query": (20, 70, 870, 470),
        "06-palette": (350, 70, 1290, 610),
        "07-context-menu": (1100, 90, 1625, 650),
        "08-columns": (450, 100, 1190, 820),
        "09-record": (130, 35, 1500, 855),
        "10-new-project": (140, 215, 1495, 685),
        "11-new-task": (140, 205, 1505, 712),
        "12-deps": (700, 105, 1632, 460),
    },
    "go": {
        "01-combobox": (640, 100, 1540, 320),
        "02-dropdown": (1060, 100, 1625, 390),
        "03-date": (1100, 90, 1632, 560),
        "04-tags": (900, 105, 1632, 320),
        "05-query": (20, 70, 870, 470),
        "06-palette": (80, 70, 1545, 560),
        "07-context-menu": (1160, 90, 1625, 740),
        "08-columns": (430, 80, 1200, 830),
        "09-record": (40, 40, 1610, 810),
        "10-new-project": (230, 225, 1420, 680),
        "11-new-task": (220, 190, 1420, 670),
        "12-deps": (700, 105, 1632, 460),
    },
}

# Tall web dialogs are cut to their top part (2x pixels).
WEB_MAXH = {"06-palette": 900}
