Name = "notifications"
NamePretty = "Notifications"
Description = "Recent desktop notifications"
Icon = "preferences-system-notifications-symbolic"

Cache = false
FixedOrder = true
HideFromProviderlist = true
History = false
HistoryWhenEmpty = false
SearchName = true
SearchPriority = { "text", "subtext", "keywords" }

local function clean(value)
  if type(value) ~= "string" then
    return ""
  end

  value = value:gsub("[%c]+", " "):gsub("%s+", " ")
  return value:match("^%s*(.-)%s*$") or ""
end

local function read_json(command)
  local handle = io.popen(command .. " 2>/dev/null")
  if not handle then
    return {}
  end

  local output = handle:read("*a")
  handle:close()

  if output == "" then
    return {}
  end

  local ok, decoded = pcall(jsonDecode, output)
  if not ok or type(decoded) ~= "table" then
    return {}
  end

  return decoded
end

local function app_name(notification)
  local app = clean(notification.app_name)
  if app == "" then
    app = clean(notification.desktop_entry)
  end

  if app:find("%.") then
    app = app:match("([^.]+)$") or app
  end

  return app:gsub("^%l", string.upper)
end

local function notification_entry(notification, kind)
  local app = app_name(notification)
  local title = clean(notification.summary)
  if title == "" then
    title = app
  end
  if title == "" then
    title = "Notification"
  end

  local body = clean(notification.body)
  local urgency = clean(notification.urgency):lower()
  if urgency ~= "critical" and urgency ~= "low" then
    urgency = "normal"
  end

  local value = title
  if body ~= "" then
    value = value .. "\n" .. body
  end

  local icon = clean(notification.app_icon)
  if icon == "" then
    icon = "preferences-system-notifications-symbolic"
  end

  return {
    Text = title,
    Subtext = body,
    Value = value,
    Icon = icon,
    Keywords = { app, title, body, urgency, kind },
    State = {
      "notification-" .. urgency,
      "notification-" .. kind,
    },
    Actions = {
      ["menus:default"] = "wl-copy",
    },
  }
end

function GetEntries()
  local entries = {}
  local seen = {}

  local function add_all(notifications, kind)
    for _, notification in ipairs(notifications) do
      local id = tostring(notification.id or "")
      local key = id
      if key == "" then
        key = kind .. ":" .. clean(notification.summary) .. ":" .. clean(notification.body)
      end

      if not seen[key] then
        seen[key] = true
        table.insert(entries, notification_entry(notification, kind))
      end
    end
  end

  add_all(read_json("makoctl list -j"), "current")
  add_all(read_json("makoctl history -j"), "history")

  if #entries == 0 then
    table.insert(entries, {
      Text = "No notifications",
      Subtext = "New notifications will appear here.",
      Icon = "preferences-system-notifications-symbolic",
      State = { "notification-empty" },
    })
  end

  return entries
end
