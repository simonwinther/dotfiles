local ls = require("luasnip")
local d = ls.dynamic_node
local sn = ls.snippet_node
local s = ls.snippet
local t = ls.text_node
local i = ls.insert_node
local f = ls.function_node
local c = ls.choice_node
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

local snippets = {
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
  -- PARAGRAPH
  s(
    { trig = "par", priority = 2000 },
    fmt([[\paragraph{<>} <>]], { i(1, "Title"), i(0) }, { delimiters = "<>" }),
    { condition = not_in_mathzone }
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
  -- MATHBB
  s({ trig = "bb", snippetType = "autosnippet" }, fmt("\\mathbb{{{}}}", { i(1) }), { condition = in_mathzone }),
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
  -- PARENTHESES: \left( ... \right)
  s(
    { trig = "prn", snippetType = "autosnippet" },
    fmt(
      [[
      \left( <> \right)
      ]],
      { i(1) },
      { delimiters = "<>" }
    ),
    { condition = in_mathzone }
  ),

  -- BRACKETS: \left[ ... \right]
  s(
    { trig = "brk", snippetType = "autosnippet" },
    fmt(
      [[
      \left[ <> \right]
      ]],
      { i(1) },
      { delimiters = "<>" }
    ),
    { condition = in_mathzone }
  ),
  -- SUMMATION
  -- 1. Full: \sum_{i=0}^{\infty}
  -- 2. Sub:  \sum_{i}
  -- 3. Sup:  \sum^{n}
  -- 4. Bare: \sum
  s(
    { trig = "sum", snippetType = "autosnippet" },
    c(1, {
      -- Full Limits
      fmt(
        [[
        \sum_{<>}^{<>}
        ]],
        { i(1, "i=0"), i(2, "\\infty") },
        { delimiters = "<>" }
      ),
      -- Subscript only
      fmt(
        [[
        \sum_{<>}
        ]],
        { i(1, "i") },
        { delimiters = "<>" }
      ),
      -- Superscript only
      fmt(
        [[
        \sum^{<>}
        ]],
        { i(1, "n") },
        { delimiters = "<>" }
      ),
      -- Bare
      fmt(
        [[
        \sum <>
        ]],
        { i(1, "x") },
        { delimiters = "<>" }
      ),
    }),
    { condition = in_mathzone }
  ),
  -- DOTS
  s({ trig = "...", snippetType = "autosnippet" }, { t("\\dots ") }, { condition = in_mathzone }),
  -- QUAD
  s({ trig = "qq", snippetType = "autosnippet" }, { t("\\quad ") }, { condition = in_mathzone }),
  -- GREATER THAN OR EQUAL
  s({ trig = "geq", snippetType = "autosnippet" }, { t("\\geq ") }, { condition = in_mathzone }),
  -- LESS THAN OR EQUAL
  s({ trig = "leq", snippetType = "autosnippet" }, { t("\\leq ") }, { condition = in_mathzone }),
  -- DELTA (Upper case)
  s({ trig = "Delta", snippetType = "autosnippet" }, { t("\\Delta ") }, { condition = in_mathzone }),

  -- delta (Lower case) - just in case you want it too
  s({ trig = "delta", snippetType = "autosnippet" }, { t("\\delta ") }, { condition = in_mathzone }),
  -- TEXTSTYLE
  s({ trig = "ts", snippetType = "autosnippet" }, { t("\\textstyle ") }, { condition = in_mathzone }),
  -- FORALL SYMBOL
  s({ trig = "fa", snippetType = "autosnippet" }, { t("\\forall ") }, { condition = in_mathzone }),
  -- THERE EXISTS (te)
  s({ trig = "te", snippetType = "autosnippet" }, { t("\\exists ") }, { condition = in_mathzone }),
  -- PRIME
  s({ trig = "pp", snippetType = "autosnippet" }, { t("\\prime") }, { condition = in_mathzone }),

  -- DELIMITER SHORTCUTS
  -- @s   set         \left\{ · \right\}
  -- @p   parentheses \left( · \right)
  -- @b   brackets    \left[ · \right]
  -- @a   angle       \left\langle · \right\rangle

  -- SET / BRACES
  s(
    { trig = "@s", snippetType = "autosnippet", wordTrig = false },
    fmt("\\left\\{ <> \\right\\}", { i(1) }, { delimiters = "<>" }),
    { condition = in_mathzone }
  ),

  -- PARENTHESES
  s(
    { trig = "@p", snippetType = "autosnippet", wordTrig = false },
    fmt("\\left( <> \\right)", { i(1) }, { delimiters = "<>" }),
    { condition = in_mathzone }
  ),

  -- BRACKETS
  s(
    { trig = "@b", snippetType = "autosnippet", wordTrig = false },
    fmt("\\left[ <> \\right]", { i(1) }, { delimiters = "<>" }),
    { condition = in_mathzone }
  ),

  -- ANGLE BRACKETS
  s(
    { trig = "@a", snippetType = "autosnippet", wordTrig = false },
    fmt("\\left\\langle <> \\right\\rangle", { i(1) }, { delimiters = "<>" }),
    { condition = in_mathzone }
  ),

  -- MULTIPLE \text{...} (e.g. 3tt → \text{…}, \text{…}, \text{…})
  -- I love this, when I do like @s 3tt
  -- Which expands to
  -- \left\{ \text{…}, \text{…}, \text{…} \right\}
  -- Feels smooth
  s(
    { trig = "(%d)tt", regTrig = true, snippetType = "autosnippet" },
    d(1, function(_, parent)
      local n = tonumber(parent.captures[1])
      local nodes = {}

      for j = 1, n do
        table.insert(nodes, t("\\text{"))
        table.insert(nodes, i(j))
        table.insert(nodes, t("}"))

        if j < n then
          table.insert(nodes, t(", "))
        end
      end

      return sn(nil, nodes)
    end),
    { condition = in_mathzone }
  ),
  -- GREEK LETTERS
  s(
    { trig = ";([a-zA-Z]+)", regTrig = true, snippetType = "autosnippet", wordTrig = false },
    f(function(_, snip)
      local match = snip.captures[1]

      local greek_map = {
        a = "alpha",
        A = "Alpha",
        b = "beta",
        B = "Beta",
        g = "gamma",
        G = "Gamma",
        d = "delta",
        D = "Delta",
        e = "epsilon",
        E = "Epsilon",
        z = "zeta",
        Z = "Zeta",
        h = "eta",
        H = "Eta",
        q = "theta",
        Q = "Theta", -- 't' is usually tau, so 'q' is theta
        i = "iota",
        I = "Iota",
        k = "kappa",
        K = "Kappa",
        l = "lambda",
        L = "Lambda",
        m = "mu",
        M = "Mu",
        n = "nu",
        N = "Nu",
        x = "xi",
        X = "Xi",
        p = "pi",
        P = "Pi",
        r = "rho",
        R = "Rho",
        s = "sigma",
        S = "Sigma",
        t = "tau",
        T = "Tau",
        u = "upsilon",
        U = "Upsilon",
        f = "phi",
        F = "Phi",
        c = "chi",
        C = "Chi",
        v = "psi",
        V = "Psi", -- 'p' is pi, so 'v' is psi
        o = "omega",
        O = "Omega",
        w = "omega",
        W = "Omega", -- Common alias

        -- Variants
        ve = "varepsilon",
        vf = "varphi",
        vk = "varkappa",
        vq = "vartheta",
        vr = "varrho",
      }

      if greek_map[match] then
        -- Returns the greek letter + a space
        return "\\" .. greek_map[match] .. " "
      else
        return ";" .. match
      end
    end),
    { condition = in_mathzone }
  ),

  -- if and only if
  s({ trig = "iff", snippetType = "autosnippet" }, { t("\\iff ") }, { condition = in_mathzone }),
  -- implies
  s({ trig = "imp", snippetType = "autosnippet" }, { t("\\implies ") }, { condition = in_mathzone }),
  -- BOLD TEXT
  s(
    { trig = "tbf", snippetType = "autosnippet" },
    fmt("\\textbf{<>}", { i(1) }, { delimiters = "<>" }),
    { condition = not_in_mathzone }
  ),
  -- TEXT ITALIC
  s(
    { trig = "tit", snippetType = "autosnippet" },
    fmt("\\textit{<>}", { i(1) }, { delimiters = "<>" }),
    { condition = not_in_mathzone }
  ),
  -- ELL SYMBOL
  s({ trig = "ll", snippetType = "autosnippet" }, { t("\\ell ") }, { condition = in_mathzone }),
  --
  s(
    { trig = "ttt", snippetType = "autosnippet" },
    fmt("\\texttt{<>}", { i(1) }, { delimiters = "<>" }),
    { condition = not_in_mathzone }
  ),
  -- MATHCAL SHORTCUT (e.g., cB -> \mathcal{B}, cC -> \mathcal{C})
  s(
    { trig = "c([A-Z])", regTrig = true, snippetType = "autosnippet" },
    f(function(_, snip)
      return "\\mathcal{" .. snip.captures[1] .. "}"
    end),
    { condition = in_mathzone }
  ),

  -- MATHBB SHORTCUT (e.g., bR -> \mathbb{R}, bZ -> \mathbb{Z})
  s(
    { trig = "b([A-Z])", regTrig = true, snippetType = "autosnippet" },
    f(function(_, snip)
      return "\\mathbb{" .. snip.captures[1] .. "}"
    end),
    { condition = in_mathzone }
  ),
}

-- AUTO-GENERATE GREEK LETTER SNIPPETS (full names like "alpha" → "\alpha ")
local greek_letters = {
  "alpha",
  "beta",
  "gamma",
  "delta",
  "epsilon",
  "zeta",
  "eta",
  "theta",
  "iota",
  "kappa",
  "lambda",
  "mu",
  "nu",
  "xi",
  "pi",
  "rho",
  "sigma",
  "tau",
  "upsilon",
  "phi",
  "chi",
  "psi",
  "omega",
  -- variants
  "varepsilon",
  "varphi",
  "varkappa",
  "vartheta",
  "varrho",
}

local greek_snippets = {}

for _, name in ipairs(greek_letters) do
  table.insert(
    greek_snippets,
    s({ trig = name, snippetType = "autosnippet" }, { t("\\" .. name .. " ") }, { condition = in_mathzone })
  )
end

vim.list_extend(snippets, greek_snippets)

return snippets
