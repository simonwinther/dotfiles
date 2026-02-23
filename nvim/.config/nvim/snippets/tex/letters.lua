local C = ...
if type(C) ~= "table" or not C.snippets then
  return {}
end

local s, t, f = C.P.s, C.P.t, C.P.f
local cond = C.cond
local snippets = C.snippets

-- GREEK LETTERS (semicolon shorthand)
table.insert(
  snippets,
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
        Q = "Theta",
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
        V = "Psi",
        o = "omega",
        O = "Omega",
        w = "omega",
        W = "Omega",

        -- Variants
        ve = "varepsilon",
        vf = "varphi",
        vk = "varkappa",
        vq = "vartheta",
        vr = "varrho",
      }

      if greek_map[match] then
        return "\\" .. greek_map[match] .. " "
      else
        return ";" .. match
      end
    end),
    { condition = cond.in_mathzone }
  )
)

-- AUTO-GENERATE GREEK LETTER SNIPPETS (full names like "alpha" -> "\alpha ")
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

for _, name in ipairs(greek_letters) do
  table.insert(
    snippets,
    s({ trig = name, snippetType = "autosnippet" }, { t("\\" .. name .. " ") }, { condition = cond.in_mathzone })
  )
end
