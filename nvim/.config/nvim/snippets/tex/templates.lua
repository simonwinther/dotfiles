local C = ...
if type(C) ~= "table" or not C.snippets then
  return {}
end

local s, i, fmt = C.P.s, C.P.i, C.P.fmt
local cond = C.cond
local snippets = C.snippets

table.insert(
  snippets,
  s(
    {
      trig = "template",
      name = "Math article template",
      dscr = "A ready-to-use article preamble for mathematical writing",
      priority = 2000,
    },
    fmt(
      [[
      \documentclass[11pt,a4paper]{article}

      \usepackage[margin=30mm]{geometry}
      \usepackage{mathtools,amssymb,amsthm}
      \usepackage{graphicx}
      \usepackage{cancel}
      \usepackage[hidelinks]{hyperref}

      \newtheorem{theorem}{Theorem}[section]
      \newtheorem{lemma}[theorem]{Lemma}
      \theoremstyle{definition}
      \newtheorem{definition}[theorem]{Definition}
      \newtheorem{problem}[theorem]{Problem}

      \title{<>}
      \author{<>}
      \date{<>}

      \begin{document}

      \maketitle

      <>

      \end{document}
      ]],
      {
        i(1, "Title"),
        i(2, "Author"),
        i(3, "\\today"),
        i(0),
      },
      { delimiters = "<>" }
    ),
    { condition = cond.not_in_mathzone }
  )
)
