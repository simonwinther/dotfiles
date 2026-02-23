local C = ...
if type(C) ~= "table" or not C.snippets then
  return {}
end

local s, t, i, d, sn = C.P.s, C.P.t, C.P.i, C.P.d, C.P.sn
local cond = C.cond
local snippets = C.snippets

-- ITEMIZE LIST
table.insert(
  snippets,
  s(
    { trig = "(%d+)item", regTrig = true, snippetType = "autosnippet" },
    d(1, function(args, parent)
      local count = tonumber(parent.captures[1])
      local nodes = {}

      table.insert(nodes, t({ "\\begin{itemize}", "" }))

      for j = 1, count do
        table.insert(nodes, t("\t\\item "))
        table.insert(nodes, i(j))
        table.insert(nodes, t({ "", "" }))
      end

      table.insert(nodes, t("\\end{itemize}"))

      return sn(nil, nodes)
    end),
    { condition = cond.not_in_mathzone }
  )
)

-- ENUMERATE LIST
table.insert(
  snippets,
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
    { condition = cond.not_in_mathzone }
  )
)
