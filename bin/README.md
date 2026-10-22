# Formula plotter

`plot` is a Rust/GTK4 replacement for the Python formula plotter. It plots
functions of `x`, with the same formula and `-r`/`--range` arguments:

```sh
plot 'sin(x), cos(x)'
plot 'y = 2x + 1' -r -5 5
plot 'x^2, sqrt(abs(x)), 3'
plot 'clip(x, -1, 1), where(x < 0, 0, x)'
```

Quote formulas so the shell passes parentheses and operators literally.
Arguments are joined with spaces, preserving the original multi-argument
invocation. Commas between formulas separate curves; commas inside function
calls stay in that function. The default x range is -10 to 10 and the initial
y range is -20 to 20, using 2000 samples per curve.

The window includes an editable formula field. Drag the graph to pan, scroll
to zoom around the pointer, and use Reset, Home, or a double-click to restore
the initial ranges. `+`/`-` zoom, arrow keys pan, Fit Y adjusts the vertical
range, and Escape closes the window. Invalid formulas appear above the graph;
valid curves still render. Undefined values and poles produce gaps.

Use Save or Ctrl+S for PNG, SVG, or PDF. Exports also work without a display:

```sh
plot 'sin(x), x^2' --output graph.png
plot 'sin(x), x^2' --output graph.svg
plot 'sin(x), x^2' --output graph.pdf
plot 'sin(x)' --width 1600 --height 1000 --output graph.png
```

PNG dimensions are pixels. SVG and PDF retain vector curves and text; PDF
uses 100 pixels per inch, so the default 1000×600 canvas is a 10×6-inch page.
Exports replace the requested file atomically after rendering succeeds.

The expression parser supports `+`, `-`, `*`, `/`, `//`, `%`, `^`, `**`,
comparisons, logical operators, parentheses, scientific notation, and implicit
multiplication (`2x`, `2(x+1)`). Constants include `pi`, `e`, `tau`, `inf`, and
`nan`. A formula evaluates one scalar y for each x; up to 16 curves can be shown.
Common elementwise functions include:

- Trigonometry: `sin`, `cos`, `tan`, `arcsin`, `arccos`, `arctan`, `arctan2`,
  hyperbolic functions and inverses, `degrees`, `radians`, and `sinc`.
- Powers and logs: `sqrt`, `cbrt`, `square`, `power`, `exp`, `exp2`, `expm1`,
  `log`/`ln`, `log2`, `log10`, and `log1p`.
- Selection and rounding: `abs`, `sign`, `minimum`, `maximum`, `clip`, `where`,
  `floor`, `ceil`, `trunc`, `rint`, `round`, and `around`.
- Other helpers: `hypot`, `mod`, `remainder`, `fmod`, `heaviside`, `isfinite`,
  `isnan`, `isinf`, and `logical_and`/`logical_or`/`logical_xor`/`logical_not`.

Standard inverse-function aliases (`asin`, `atan`, etc.) and `np.`/`numpy.`
prefixes are accepted. `minimum(x, 2)` is an elementwise comparison;
`min(x, 2)` and `max(x, 2)` are scalar aliases. Python array constructors,
indexing, imports, and arbitrary Python expressions are outside this interface.

## Build and install

Run `stow bin` and `plot-build` from this checkout on each PC. Building needs
Rust/Cargo 1.88+, a C compiler, pkg-config, GTK4 4.8+, and Cairo with its PDF
backend (standard Arch GTK4/Cairo packages). The runtime needs GTK4/Cairo;
NumPy, Matplotlib, and Python are no longer required by `plot`.

Source is in `bin/.local/share/plot`. The launchers are in `bin/.local/bin`.
Cargo's lockfile, build files, and native executable stay in
`${XDG_CACHE_HOME:-~/.cache}/plot`, outside the repository. Rebuild after source
changes with `plot-build`; `plot-build --check` also runs formatting, Clippy,
unit tests, and headless command-line/export tests.

The GTK window draws with the same scene used by exports. The drawing API is
[GTK's DrawingArea](https://gtk-rs.org/gtk4-rs/stable/0.9/docs/gtk4/struct.DrawingArea.html);
PDF output uses [Cairo PDF surfaces](https://www.cairographics.org/manual/cairo-PDF-Surfaces.html).
