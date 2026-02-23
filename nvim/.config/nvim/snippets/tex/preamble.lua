if ... ~= "_load" then
  return {}
end

local ls = require("luasnip")

return {
  ls = ls,
  s = ls.snippet,
  t = ls.text_node,
  i = ls.insert_node,
  f = ls.function_node,
  c = ls.choice_node,
  d = ls.dynamic_node,
  sn = ls.snippet_node,
  fmt = require("luasnip.extras.fmt").fmt,
}
