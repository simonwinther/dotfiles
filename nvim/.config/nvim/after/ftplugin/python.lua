local core = require("utils.core")

local function get_terminal_window(name)
  local buf = vim.fn.bufnr(name)

  -- Wipe buffer if it exists but is 'dead'
  if buf ~= -1 and not vim.api.nvim_buf_is_valid(buf) then
    vim.cmd("bwipeout! " .. buf)
    buf = -1
  end

  if buf == -1 then
    buf = vim.api.nvim_create_buf(false, true)
    vim.api.nvim_buf_set_name(buf, name)
  end

  -- Open split if closed, otherwise focus it
  if vim.fn.bufwinid(buf) == -1 then
    vim.cmd("botright vsplit")
    vim.api.nvim_win_set_buf(0, buf)
  else
    vim.api.nvim_set_current_win(vim.fn.bufwinid(buf))
  end
end

local function run_python(is_visual)
  local path = vim.api.nvim_buf_get_name(0)
  local is_temp = false

  if is_visual then
    -- Make sure core.get_visual_selection is available
    local lines = core.get_visual_selection()
    path = vim.fn.tempname() .. ".py"
    vim.fn.writefile(lines, path)
    is_temp = true
  else
    vim.cmd("write")
    if path == "" then
      return print("Error: Save file first")
    end
  end

  get_terminal_window("PYTHON_RUNNER")

  vim.fn.jobstart({ "python3", "-u", path }, {
    term = true,
    on_exit = function(_, code)
      if code ~= 0 then
        print("Exit Code: " .. code)
      end
      if is_temp then
        vim.fn.delete(path)
      end
    end,
  })

  vim.cmd("startinsert")
end

-- Buffer-local mappings
vim.keymap.set("n", "<localleader>r", function()
  run_python(false)
end, { buffer = true, desc = "Run File" })
vim.keymap.set("v", "<localleader>r", function()
  run_python(true)
end, { buffer = true, desc = "Run Selection" })
