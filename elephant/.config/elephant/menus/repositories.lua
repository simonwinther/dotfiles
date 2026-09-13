Name = "repositories"
NamePretty = "Repositories"
Icon = "GitHub"
Cache = false
Action = "xdg-open '%VALUE%'"

local default_owner = "simonwinther"

local function is_valid_path_component(value)
  return value:match("^[A-Za-z0-9_.-]+$") and value ~= "." and value ~= ".."
end

function GetEntries(query)
  local input = (query or ""):match("^%s*(.-)%s*$")

  if input == "" then
    return {
      {
        Text = "My repositories",
        Subtext = "Type a repository, or a user and repository",
        Value = "https://github.com/" .. default_owner .. "?tab=repositories",
      },
    }
  end

  local parts = {}
  for part in input:gmatch("%S+") do
    table.insert(parts, part)
  end

  if #parts > 2 then
    return {}
  end

  local owner = #parts == 2 and parts[1] or default_owner
  local repository = parts[#parts]

  if not is_valid_path_component(owner) or not is_valid_path_component(repository) then
    return {}
  end

  local repository_path = owner .. "/" .. repository
  local url = "https://github.com/" .. repository_path

  return {
    {
      Text = repository_path,
      Subtext = url,
      Value = url,
      Keywords = { input },
    },
  }
end
