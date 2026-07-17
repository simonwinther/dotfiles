local ls = require("luasnip")
local s = ls.snippet
local i = ls.insert_node
local fmt = require("luasnip.extras.fmt").fmt

return {
  s(
    {
      trig = "template",
      name = "Todo template",
      dscr = "A printable Typst todo list",
    },
    fmt(
      [[
      #set page(
        paper: "a4",
        margin: 2cm,
      )

      #set text(size: 14pt, fill: rgb("#20242b"))

      #let accent = rgb("#3b5f8a")

      #align(center)[
        #text(
          size: 24pt,
          weight: "bold",
          tracking: 0.08em,
          fill: accent,
        )[<>]

        #v(5pt)
        #line(length: 52pt, stroke: 2pt + accent)
      ]

      #v(1.25em)

      #let todo(body) = [
        #grid(
          columns: (14pt, 1fr),
          column-gutter: 8pt,
          align: horizon,
          box(
            width: 12pt,
            height: 12pt,
            radius: 2pt,
            stroke: 1.2pt + accent,
          ),
          body,
        )
        #v(10pt)
      ]

      <>
      ]],
      {
        i(1, "TODO"),
        i(0),
      },
      { delimiters = "<>" }
    )
  ),
}
