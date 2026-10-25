Tidewater
=========

A reimplementation of the Warcraft II: Battle.net Edition engine. It contains no game data: you need your own copy
of the game (for example the GOG release).

Running it
----------
1. Unpack this archive anywhere you like.
2. Run ./tidewater (from a terminal, or double-click it in your file manager).
3. The first time, Tidewater opens an import screen. It lists the game installs it found; choose one, or use the
   buttons to pick a game folder, a GOG installer (setup_warcraft_ii_*.exe) or a CD image (.iso, .cue/.bin), or drop
   one on the window. It copies the files it needs once, then starts the game.

To import again later: ./tidewater --reimport

Where things are kept
---------------------
Imported game files, saved games and settings live in your own data folder, ~/.local/share/tidewater (or under
$XDG_DATA_HOME). Nothing is written next to the program or into your game install. Delete that folder to remove them.

A menu entry
------------
Edit the Exec line of tidewater.desktop to the full path of ./tidewater, then copy the file to
~/.local/share/applications/. For its icon, copy tidewater.png to ~/.local/share/icons/hicolor/256x256/apps/ (or
change the Icon line to the full path of tidewater.png).

Updating
--------
The main menu offers "Update now" when a newer release exists; on Linux that opens the releases page, where you
download the new archive and unpack it over this one. Turn the check off with --no-update-check.

Checking the download
---------------------
Each release lists SHA256SUMS; run sha256sum -c SHA256SUMS --ignore-missing next to the downloaded files.
