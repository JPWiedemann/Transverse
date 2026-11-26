Transverse - transmission line cross-section solver
===================================================

transverse-gui   (transverse-gui.exe on Windows)
    The desktop application. No installation: unpack the folder anywhere and start it.
    Your inputs are saved in settings.json next to the program; delete it to reset them.

transverse       (transverse.exe on Windows)
    Command line: batch analysis and a self-test against reference results.

        transverse selftest
        echo {} | transverse analyze
        echo {} | transverse synth w 35
        echo {"f":2.4e9} | transverse synth w 35 --phase 90

Linux: needs a desktop with OpenGL (Mesa is fine) and X11 or Wayland.
Windows: needs nothing besides OpenGL 2.0 or newer (any normal graphics driver).

License
-------
Transverse is free software: GNU General Public License, version 3 only (see LICENSE).
Source code: https://github.com/JPWiedemann/Transverse
Licenses of the bundled third-party libraries: THIRD_PARTY_LICENSES.txt
