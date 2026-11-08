-- Tags text that is a single http(s) link, and records its host as a second tag.
local tag = "url"

return {
  id = "example.lua-urltag",
  version = "1.0.0",
  api_version = 1,
  capabilities = { "processor" },

  init = function(host, config)
    for key in pairs(config) do
      if key ~= "tag" then error("unknown config key") end
    end
    if config.tag ~= nil then tag = config.tag end
  end,

  process = function(ctx)
    if ctx.item.type ~= "text" then return end
    local text = ctx.item.text:match("^%s*(.-)%s*$")
    if text:find("%s") then return end
    local host = text:match("^https?://([^/%?#:]+)")
    if host then
      ctx:add_tag(tag)
      ctx:add_tag("host:" .. host:lower())
    end
  end,
}
