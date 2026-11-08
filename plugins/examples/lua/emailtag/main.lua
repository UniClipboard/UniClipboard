-- Minimal Lua plugin template: tags text that contains an email address.
-- The returned table must repeat the declaration the host gives the engine.
local tag = "email"

return {
  id = "example.lua-emailtag",
  version = "1.0.0",
  api_version = 1,
  capabilities = { "processor" },

  -- Reject keys you do not know: configuration is strict.
  init = function(host, config)
    for key in pairs(config) do
      if key ~= "tag" then error("unknown config key") end
    end
    if config.tag ~= nil then tag = config.tag end
  end,

  process = function(ctx)
    if ctx.item.type == "text" and ctx.item.text:find("[%w._-]+@[%w-]+%.[%w.]+") then
      ctx:add_tag(tag)
    end
  end,
}
