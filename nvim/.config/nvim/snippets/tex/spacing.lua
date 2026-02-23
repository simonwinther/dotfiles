local C = ...
if type(C) ~= "table" or not C.snippets then
  return {}
end

local s, t = C.P.s, C.P.t
local cond = C.cond
local snippets = C.snippets

-- TEXTSTYLE
table.insert(
  snippets,
  s({ trig = "ts", snippetType = "autosnippet" }, { t("\\textstyle ") }, { condition = cond.in_mathzone })
)

-- MEDSKIP
table.insert(snippets, s({ trig = "ms" }, { t("\\medskip") }, { condition = cond.not_in_mathzone }))

-- MEDSKIP + NOINDENT
table.insert(snippets, s({ trig = "mn" }, { t({ "\\medskip", "\\noindent " }) }, { condition = cond.not_in_mathzone }))

-- NOINDENT
table.insert(snippets, s("ni", { t({ "\\noindent" }) }, { condition = cond.not_in_mathzone }))

-- NEWLINE
table.insert(snippets, s("nl", { t({ "\\newline" }) }, { condition = cond.not_in_mathzone }))
