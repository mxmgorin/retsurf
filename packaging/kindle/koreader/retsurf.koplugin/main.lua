--[[--
Starts the Retsurf browser from the Tools menu. KOReader holds the panel and
grabs the touch device, so it quits first; from-koreader.sh waits for that,
runs the browser, and starts KOReader again.

@module koplugin.Retsurf
--]]--

local Device = require("device")
local UIManager = require("ui/uimanager")
local WidgetContainer = require("ui/widget/container/widgetcontainer")
local _ = require("gettext")

local RETSURF_DIR = "/mnt/us/retsurf"

local Retsurf = WidgetContainer:extend{
    name = "retsurf",
    is_doc_only = false,
}

function Retsurf:init()
    if not Device:isKindle() then
        return
    end
    self.ui.menu:registerToMainMenu(self)
end

function Retsurf:addToMainMenu(menu_items)
    menu_items.retsurf = {
        text = _("Retsurf browser"),
        sorting_hint = "tools",
        callback = function()
            os.execute("setsid " .. RETSURF_DIR .. "/from-koreader.sh </dev/null >/dev/null 2>&1 &")
            UIManager:quit()
        end,
    }
end

return Retsurf
