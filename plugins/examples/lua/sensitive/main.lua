-- Marks likely secrets, keeps them out of sync and out of the history after
-- ten minutes.
local ten_minutes = 10 * 60 * 1000

local function looks_secret(text)
  if #text > 4096 then return false end
  return text:find("%-%-%-%-%-BEGIN [%u ]*PRIVATE KEY%-%-%-%-%-") ~= nil
      or text:find("[Pp]assword%s*[:=]") ~= nil
      or text:find("sk%-[%w]+") ~= nil
end

return {
  id = "example.lua-sensitive",
  version = "1.0.0",
  api_version = 1,
  capabilities = { "processor" },
  process = function(ctx)
    if ctx.item.type ~= "text" then return end
    if looks_secret(ctx.item.text) then
      ctx:add_tag("sensitive")
      ctx:set_sync(false)
      ctx:set_expire(ten_minutes)
    end
  end,
}
