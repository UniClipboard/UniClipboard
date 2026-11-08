-- A user-triggered action: produces a new history item with the text in
-- title case. Actions never run automatically.
return {
  id = "example.lua-textcase",
  version = "1.0.0",
  api_version = 1,
  capabilities = { "action" },
  actions = { { id = "title", title = "Title Case", accepts = { "text" } } },
  run_action = function(ctx)
    local out = ctx.item.text:lower():gsub("(%a)([%w']*)", function(a, b) return a:upper() .. b end)
    ctx:set_text(out)
  end,
}
