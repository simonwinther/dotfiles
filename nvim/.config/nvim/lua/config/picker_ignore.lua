local M = {}

local picker_ignore_file = vim.fs.joinpath(vim.fn.stdpath("config"), "picker-ignore")

local function normalize_cwd(cwd)
  cwd = cwd or vim.uv.cwd() or vim.fn.getcwd()
  return vim.fs.normalize(vim.fn.fnamemodify(cwd, ":p"))
end

function M.paths(cwd)
  local paths = {}

  if vim.fn.filereadable(picker_ignore_file) == 1 then
    table.insert(paths, picker_ignore_file)
  end

  local project_ignore = vim.fs.joinpath(normalize_cwd(cwd), ".grepignore")
  if vim.fn.filereadable(project_ignore) == 1 then
    table.insert(paths, project_ignore)
  end

  return paths
end

local function has_ignore_file(args, path)
  for index, arg in ipairs(args) do
    if arg == "--ignore-file" and args[index + 1] == path then
      return true
    end
  end

  return false
end

function M.extend(args, cwd)
  for _, path in ipairs(M.paths(cwd)) do
    if not has_ignore_file(args, path) then
      vim.list_extend(args, { "--ignore-file", path })
    end
  end

  return args
end

function M.shell_argv(cwd)
  local args = {}

  for _, path in ipairs(M.paths(cwd)) do
    vim.list_extend(args, { "--ignore-file", vim.fn.shellescape(path) })
  end

  return args
end

return M
