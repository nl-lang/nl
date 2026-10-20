# Number interpolation and angles: lerp, remap, approach, normalize_angle, angle_difference, approach_angle
print 0.lerp(10, 0.25), 10.lerp(20, 0), 10.lerp(20, 1), 0.lerp(10, 1.5)
print Number.lerp(-4, 4, 0.5)

print 5.remap(0, 10, 0, 100), 15.remap(10, 20, 1, 0), 30.remap(0, 10, 0, 1)

print 0.approach(10, 3), 9.approach(10, 3), 10.approach(0, 4), 2.approach(0, 5), 5.approach(5, 1)
print 0.approach(10, -3)

# Angles in degrees, normalized to [-180, 180)
print 190.normalize_angle, 180.normalize_angle, (-190).normalize_angle, 720.normalize_angle, 45.normalize_angle
print 350.angle_difference(10), 10.angle_difference(350), 90.angle_difference(90)
print 350.approach_angle(10, 15), 350.approach_angle(10, 50), 10.approach_angle(350, 5)

# A value moving toward a target over a few steps
let x = 0
let steps = []
until x == 7
  x = x.approach(7, 2)
  Table.insert(steps, x)
end
print Table.concat(steps, " ")

print 3.hypot(4), (-5).hypot(12), 0.hypot(0), 1e200.hypot(1e200) > 1e200
