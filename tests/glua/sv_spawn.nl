# Server: a loadout on spawn, a chat command and scaled damage, with
# gmod's API under its NL names (decl/glua.d.nl).

const LOADOUT = ["weapon_crowbar", "weapon_pistol", "weapon_physgun"]

fn give_loadout(ply: Player)
  ply.strip_weapons
  for class in LOADOUT
    ply.give class
  end
  ply.give_ammo 60, "Pistol"
  ply.set_health 150
  ply.set_armor 25
  ply.select_weapon "weapon_pistol"
end

Hook.add "PlayerSpawn", "nl_loadout", fn(ply: Player, transition: Bool)
  return if transition
  Timer.simple 0 do
    give_loadout ply if ply.is_valid
  end
end

Hook.add "PlayerSay", "nl_commands", fn(ply: Player, text: String, team_only: Bool)
  if text == "!heal" and ply.alive
    ply.set_health ply.get_max_health
    ply.chat_print "Healed, #{ply.nick}."
    return ""
  end
  if text == "!where"
    let pos = ply.get_pos
    ply.chat_print "You are at #{pos.x}, #{pos.y}, #{pos.z} (#{team_only})"
    return ""
  end
end

Hook.add "EntityTakeDamage", "nl_scale", fn(target: Entity, info: CTakeDamageInfo)
  if target.is_player and info.is_damage_type(DMG_FALL)
    info.scale_damage 0.5
  end
  let attacker = info.get_attacker
  if attacker.is_valid and attacker.is_npc
    info.set_damage info.get_damage * 2
  end
end

for ply in PlayerLib.get_all()
  print "#{ply.nick} (#{ply.steam_id}) has #{ply.health} health"
end

print "players: #{PlayerLib.get_count()} of #{Game.max_players()}"
print "map: #{Game.get_map()}, server: #{SERVER}, gamemode: #{GAMEMODE.name}"
