local C = ...
if type(C) ~= "table" or not C.snippets then
  return {}
end

local s, i, f, fmt = C.P.s, C.P.i, C.P.f, C.P.fmt
local cond = C.cond
local helpers = C.helpers
local snippets = C.snippets

-- SECTION
table.insert(
  snippets,
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
        lbl = f(helpers.to_label, { 1 }),
        final = i(0),
        lbl_mirror = f(helpers.to_label, { 1 }),
      }
    )
  )
)

-- SUBSECTION
table.insert(
  snippets,
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
        lbl = f(helpers.to_label, { 1 }),
        final = i(0),
        lbl_mirror = f(helpers.to_label, { 1 }),
      }
    )
  )
)

-- SUBSUBSECTION
table.insert(
  snippets,
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
        lbl = f(helpers.to_label, { 1 }),
        final = i(0),
        lbl_mirror = f(helpers.to_label, { 1 }),
      }
    )
  )
)

-- PARAGRAPH
table.insert(
  snippets,
  s(
    { trig = "par", priority = 2000 },
    fmt([[\paragraph{<>} <>]], { i(1, "Title"), i(0) }, { delimiters = "<>" }),
    { condition = cond.not_in_mathzone }
  )
)
