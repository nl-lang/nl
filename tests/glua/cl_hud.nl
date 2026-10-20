# Client: a HUD element drawn every frame.

Surface.create_font "NLHud", {font: "Roboto", size: 24, weight: 600}

let accent = color(80, 160, 255)

Hook.add "HUDPaint", "nl_hud", fn
  let ply = local_player()
  return unless ply.is_valid and ply.alive
  let w = scr_w()
  let h = scr_h()
  let hp = Number.max(ply.health, 0)
  let frac = Number.min(hp / ply.get_max_health, 1)

  Surface.set_draw_color 0, 0, 0, 180
  Surface.draw_rect 20, h - 60, 220, 30
  Surface.set_draw_color accent.r, accent.g, accent.b, 255
  Surface.draw_rect 22, h - 58, Number.floor(216 * frac), 26
  Draw.simple_text "#{hp} HP", "NLHud", 130, h - 45, color_white, TEXT_ALIGN_CENTER, TEXT_ALIGN_CENTER

  let wep = ply.get_active_weapon
  if wep.is_valid
    Draw.simple_text "#{wep.clip1} / #{ply.get_ammo_count(wep.get_primary_ammo_type)}", "NLHud", w - 30, h - 45, color_white, TEXT_ALIGN_RIGHT, TEXT_ALIGN_CENTER
  end

  let tr = ply.get_eye_trace
  let target = tr.Entity
  if target is Player
    let pos = (target.get_pos + vector(0, 0, 80)).to_screen
    Draw.simple_text target.nick, "NLHud", pos.x, pos.y, Team.get_color(target.team), TEXT_ALIGN_CENTER
  end
end

Hook.add "HUDShouldDraw", "nl_hide", fn(name: String)
  return false if name == "CHudHealth"
end

# Lua's string functions through gmod's `string`: a replacement can be a
# function or a table, and find gives nil when nothing matches.
fn shout_tags(text: String): String
  let out, count = StringLib.gsub(text, "<(%w+)>", fn(tag) tag.upcase end)
  print "#{count} tags"
  StringLib.gsub(out, "%$(%w+)", {hp: "100"})
end

fn has_tag(text: String): Bool
  let at = StringLib.find(text, "<", 1, true)
  at != nil
end

print shout_tags("<b>$hp</b>"), has_tag("plain")
