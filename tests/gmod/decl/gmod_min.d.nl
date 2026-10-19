## A few of gmod's classes, for the programs of tests/gmod (the full
## declarations are decl/glua.d.nl).
declare type Entity
  fn ent_index(self): Number as 'EntIndex'
  fn get_class(self): String as 'GetClass'
end

declare type Player : Entity
  fn nick(self): String as 'Nick'
end

declare type Weapon : Entity
end

declare fn player_by_id(userid: Number): Player = :Player
declare fn entity_by_index(index: Number): Entity = :Entity
