# Temml source

TeX-to-MathML renderer from [ronkok/Temml](https://github.com/ronkok/Temml), pinned to commit `4ffdf0e56577c426375d0b2818f8895edb5e5030` (version 0.14.0). See the included MIT license.

These are the original `temml.js` and `src/**/*.js` source modules. Relative module imports have explicit `.js` extensions so Chrome can load them directly. There is no bundled or minified build, binary asset, or build step.

`math.css` comes from the same commit's `site/assets/Temml-Local.css`, with the web font declaration removed and the Temml font references replaced by the browser's native `math` font. Math uses installed system fonts.

The reader calls `render` directly with `trust: false`, bounded macro expansion, and visible fallback for invalid expressions. It does not use the upstream auto-render entry point or load remote code or fonts.
