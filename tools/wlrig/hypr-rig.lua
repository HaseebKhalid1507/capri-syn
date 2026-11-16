-- The rig's Hyprland (BRIEF-wayland §4.4): Haseeb's look and input (hyprland.lua, read-only copy of the
-- relevant parts). WAYLAND-1 stands in for eDP-1; `hyprctl output create headless HEADLESS-2` adds DP-2's
-- stand-in; a bar is a wlworld.py panel (a layer with an exclusive zone, as Caelestia's is).
-- No require("nvidia"), no hl.env, no start hooks, no idle daemon, no shell.
hl.monitor({ output = "WAYLAND-1",  mode = "2560x1600", position = "0x0",    scale = 1 })
hl.monitor({ output = "HEADLESS-2", mode = "2560x1440", position = "2560x0", scale = 1 })
hl.monitor({ output = "",           mode = "preferred", position = "auto",   scale = 1 })

hl.config({
    input = { kb_layout = "us", follow_mouse = 1, sensitivity = 0 },
    general = {
        gaps_in = 6, gaps_out = 14, border_size = 0, layout = "dwindle", resize_on_border = true, allow_tearing = true,
        col = { active_border = { colors = { "rgba(c2c1ffee)", "rgba(f5b2e0ee)" }, angle = 45 }, inactive_border = "rgba(47464faa)" },
    },
    decoration = {
        rounding = 14, active_opacity = 1.0, inactive_opacity = 0.92, dim_inactive = true, dim_strength = 0.12,
        blur = { enabled = true, size = 8, passes = 3, new_optimizations = true, ignore_opacity = true, xray = false, vibrancy = 0.1696 },
        shadow = { enabled = true, range = 30, render_power = 3, color = "rgba(11111baa)" },
    },
    animations = { enabled = true },
    dwindle = { preserve_split = true },
    -- enable_anr_dialog: the rig's test toplevels (wlworld.py) answer pings only while a script dispatches,
    -- and the "not responding" overlay would land in captures.
    misc = { force_default_wallpaper = 1, disable_hyprland_logo = true, enable_anr_dialog = false },
    render = { direct_scanout = false },
})

hl.curve("wind",      { type = "bezier", points = { {0.05, 0.9},  {0.1, 1.05}   } })
hl.curve("overshot",  { type = "bezier", points = { {0.13, 0.99}, {0.29, 1.1}   } })
hl.curve("smoothOut", { type = "bezier", points = { {0.36, 0},    {0.66, -0.56} } })
hl.curve("smoothIn",  { type = "bezier", points = { {0.25, 1},    {0.5, 1}      } })
hl.animation({ leaf = "windows",     enabled = true, speed = 6,  bezier = "overshot",  style = "slide" })
hl.animation({ leaf = "windowsOut",  enabled = true, speed = 5,  bezier = "smoothOut", style = "slide" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 5,  bezier = "wind",      style = "slide" })
hl.animation({ leaf = "border",      enabled = true, speed = 10, bezier = "default" })
hl.animation({ leaf = "borderangle", enabled = true, speed = 30, bezier = "default",   style = "loop" })
hl.animation({ leaf = "fade",        enabled = true, speed = 7,  bezier = "smoothIn" })
hl.animation({ leaf = "workspaces",  enabled = true, speed = 6,  bezier = "overshot",  style = "slidefade 20%" })

-- Appendix A (avatarctl in place of capri-ctl): SUPER+C tap = the Line, hold = talk; her layers never animate
-- (the match is on the whole namespace: "^capri-" alone matches nothing, and her margin moves slide).
hl.bind("SUPER + C", hl.dsp.exec_cmd("avatarctl talk press"),   { description = "Capri: Line / talk" })
hl.bind("SUPER + C", hl.dsp.exec_cmd("avatarctl talk release"), { release = true, description = "Capri: talk release" })
hl.layer_rule({ name = "capri-noanim", match = { namespace = "^capri-.*$" }, no_anim = true })
