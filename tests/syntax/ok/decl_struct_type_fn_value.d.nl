# Declare struct (a Lua class) with operators, declare type with a parent, declare fn, let and const
declare struct Point
  x: Number
  y: Number
  fn new(x: Number?, y: Number?): Point
  fn length(self): Number
  fn +(right: Point): Point
  fn -: Point
  fn len: Number
  fn [](key: Number): Number
end

declare type Player : Entity
  fn steam_id(self): String as 'SteamID'
  fn pos(self): Vector as 'GetPos'
end

declare type Entity
end

declare fn local_player(): Player = :LocalPlayer
declare fn find<T>(items: [T], *rest: [T], **opts: {limit: Number?}): (T?, Number)
declare let frame_count: Number = :game.frameCount
declare const max_players: Number = :config.maxPlayers
declare const MAX_HP: Number
