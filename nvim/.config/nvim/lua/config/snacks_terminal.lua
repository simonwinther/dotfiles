local M = {}
local configured = false

function M.setup()
  if configured or not vim.env.TMUX then
    return
  end
  configured = true

  -- Snacks handles extended-keys "on", but misses "always". The resulting
  -- terminal version reply can become input (including paste commands).
  -- https://github.com/folke/snacks.nvim/issues/2332
  local terminal = require("snacks.image.terminal")
  local detect = terminal._detect
  terminal._detect = function(cb)
    if not terminal._terminal then
      local mode = vim.fn.system({ "tmux", "show", "-gv", "extended-keys" })
      if vim.v.shell_error == 0 and vim.trim(mode) == "always" then
        local name = vim.fn.system({ "tmux", "display-message", "-p", "#{client_termname}" })
        name = vim.trim(name)
        if vim.v.shell_error == 0 and name ~= "" then
          pcall(vim.fn.system, { "tmux", "set", "-p", "allow-passthrough", "all" })
          terminal.transform = function(data)
            return ("\027Ptmux;" .. data:gsub("\027", "\027\027")) .. "\027\\"
          end
          terminal._terminal = {
            terminal = name:gsub("^xterm%-", ""),
            version = "unknown",
          }
        end
      end
    end
    return detect(cb)
  end
end

return M
