local ls = require("luasnip")
local d = ls.dynamic_node
local sn = ls.snippet_node
local s = ls.snippet
local t = ls.text_node
local i = ls.insert_node
local f = ls.function_node
local fmt = require("luasnip.extras.fmt").fmt

----------------------------------------
--- CONDITIONS (MATH MODE)
----------------------------------------

local function in_mathzone()
  return vim.fn["vimtex#syntax#in_mathzone"]() == 1
end

local function not_in_mathzone()
  return vim.fn["vimtex#syntax#in_mathzone"]() == 0
end

----------------------------------------
--- HELPER FUNCTIONS
----------------------------------------

local function to_label(args)
  return args[1][1]:lower():gsub("%p", ""):gsub("%s+", "_")
end

return {
  -- SECTION
  s(
    "sec",
    fmt(
      [[
    \section{{{title}}}\label{{sec:{lbl}}} % (fold)

    {final}

    % section {lbl_mirror} (end)
    ]],
      {
        title = i(1, "Title"),
        lbl = f(to_label, { 1 }),
        final = i(0),
        lbl_mirror = f(to_label, { 1 }),
      }
    )
  ),

  -- SUBSECTION
  s(
    "sub",
    fmt(
      [[
    \subsection{{{title}}}\label{{sub:{lbl}}} % (fold)

    {final}

    % subsection {lbl_mirror} (end)
    ]],
      {
        title = i(1, "Title"),
        lbl = f(to_label, { 1 }),
        final = i(0),
        lbl_mirror = f(to_label, { 1 }),
      }
    )
  ),

  -- SUBSUBSECTION
  s(
    "ssub",
    fmt(
      [[
    \subsubsection{{{title}}}\label{{ssub:{lbl}}} % (fold)

    {final}

    % subsubsection {lbl_mirror} (end)
    ]],
      {
        title = i(1, "Title"),
        lbl = f(to_label, { 1 }),
        final = i(0),
        lbl_mirror = f(to_label, { 1 }),
      }
    )
  ),
  -- ITEMIZE LIST
  s(
    { trig = "(%d+)item", regTrig = true, snippetType = "autosnippet" },
    d(1, function(args, parent)
      -- Capture the number from the trigger
      local count = tonumber(parent.captures[1])
      local nodes = {}

      -- Start the environment
      table.insert(nodes, t({ "\\begin{itemize}", "" }))

      -- Loop to generate items
      for j = 1, count do
        table.insert(nodes, t("\t\\item "))
        table.insert(nodes, i(j)) -- Create a jump point for each item
        table.insert(nodes, t({ "", "" })) -- Add a newline
      end

      -- End the environment
      table.insert(nodes, t("\\end{itemize}"))

      return sn(nil, nodes)
    end),
    { condition = not_in_mathzone }
  ),

  -- ENUMERATE LIST
  s(
    { trig = "(%d+)enum", regTrig = true, snippetType = "autosnippet" },
    d(1, function(args, parent)
      local count = tonumber(parent.captures[1])
      local nodes = {}

      table.insert(nodes, t({ "\\begin{enumerate}", "" }))

      for j = 1, count do
        table.insert(nodes, t("\t\\item "))
        table.insert(nodes, i(j))
        table.insert(nodes, t({ "", "" }))
      end

      table.insert(nodes, t("\\end{enumerate}"))

      return sn(nil, nodes)
    end),
    { condition = not_in_mathzone }
  ),
  -- FRACTION
  s(
    { trig = "//", snippetType = "autosnippet" },
    fmt("\\frac{<>}{<>}", {
      i(1),
      i(2),
    }, { delimiters = "<>" }),
    { condition = in_mathzone }
  ),
  -- SUPERSCRIPT
  s(
    { trig = "^", snippetType = "autosnippet", wordTrig = false },
    fmt("^{{{}}}", { i(1) }),
    { condition = in_mathzone }
  ),

  -- SUBSCRIPT
  s(
    { trig = "_", snippetType = "autosnippet", wordTrig = false },
    fmt("_{{{}}}", { i(1) }),
    { condition = in_mathzone }
  ),

  -- CROSS PRODUCT
  s({ trig = "xx", snippetType = "autosnippet" }, { t("\\times ") }, { condition = in_mathzone }),

  -- DOT PRODUCT
  s({ trig = "**", snippetType = "autosnippet" }, { t("\\cdot ") }, { condition = in_mathzone }),

  -- EQUATION
  s(
    "eq",
    fmt(
      [[
      \begin{equation}
          <>
      \end{equation}
      ]],
      { i(1) },
      { delimiters = "<>" } -- <--- THIS WAS MISSING
    ),
    { condition = not_in_mathzone }
  ),

  -- EQUATION
  s(
    { trig = "eqq", wordTrig = false },
    fmt(
      [[
      \begin{equation*}
          <>
      \end{equation*}
      ]],
      { i(1) },
      { delimiters = "<>" }
    ),
    { condition = not_in_mathzone }
  ),
  -- ALIGN (Numbered)
  -- Trigger: al
  s(
    "al",
    fmt(
      [[
      \begin{align}
          <>
      \end{align}
      ]],
      { i(1) },
      { delimiters = "<>" }
    ),
    { condition = not_in_mathzone }
  ),

  -- ALIGN
  s(
    "all",
    fmt(
      [[
      \begin{align*}
          <>
      \end{align*}
      ]],
      { i(1) },
      { delimiters = "<>" }
    ),
    { condition = not_in_mathzone }
  ),
  -- MEDSKIP + NOINDENT
  s({ trig = "mn" }, {
    t({ "\\medskip", "\\noindent " }),
  }, { condition = not_in_mathzone }),

  -- MEDSKIP
  s({ trig = "ms" }, {
    t("\\medskip"),
  }, { condition = not_in_mathzone }),
  -- NOINDENT
  s("ni", {
    t({ "\\noindent" }),
  }, { condition = not_in_mathzone }),

  -- NEWLINE
  s("nl", {
    t({ "\\newline" }),
  }, { condition = not_in_mathzone }),
  -- NOT EQUAL
  s({ trig = "neq", snippetType = "autosnippet" }, { t("\\neq ") }, { condition = in_mathzone }),
  -- NABLA
  s({ trig = "nbla", snippetType = "autosnippet" }, { t("\\nabla ") }, { condition = in_mathzone }),
  -- MU
  s({ trig = "mu", snippetType = "autosnippet" }, { t("\\mu ") }, { condition = in_mathzone }),
  -- PI
  s({ trig = "pi", snippetType = "autosnippet" }, { t("\\pi ") }, { condition = in_mathzone }),
  -- IN
  s({ trig = "in", snippetType = "autosnippet" }, { t("\\in ") }, { condition = in_mathzone }),
  -- PI (P)
  s({ trig = "Pi", snippetType = "autosnippet" }, { t("\\Pi ") }, { condition = in_mathzone }),
  -- TEXT
  s({ trig = "tt", snippetType = "autosnippet" }, fmt("\\text{{{}}}", { i(1) }), { condition = in_mathzone }),
  -- MATHCAL
  s({ trig = "cal", snippetType = "autosnippet" }, fmt("\\mathcal{{{}}}", { i(1) }), { condition = in_mathzone }),
  -- SET
  s(
    { trig = "set", snippetType = "autosnippet" },
    fmt(
      [[
      \left\{ <> \right\}
      ]],
      { i(1) },
      { delimiters = "<>" } -- Use <> to avoid clashing with LaTeX {}
    ),
    { condition = in_mathzone }
  ),
  -- DOTS
  s({ trig = "...", snippetType = "autosnippet" }, { t("\\dots ") }, { condition = in_mathzone }),
}
