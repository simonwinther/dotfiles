local ls = require("luasnip")
local s = ls.snippet
local i = ls.insert_node
local f = ls.function_node
local fmt = require("luasnip.extras.fmt").fmt

----------------------------------------
--- HELPER FUNCTIONS :)
----------------------------------------

-- Helper function: Converts "My Title" -> "my_title"
local function to_label(args)
  -- args[1][1] is the text content of the first node linked to this function
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
}
