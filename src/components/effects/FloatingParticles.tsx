interface Particle {
  id: number
  size: number
  left: number
  delay: number
  duration: number
  opacity: number
  drift: number
}

// Pre-computed particle data to avoid Math.random() during render
const PARTICLES: Particle[] = [
  { id: 0, size: 4.2, left: 8, delay: 2.1, duration: 18, opacity: 0.45, drift: 25 },
  { id: 1, size: 3.5, left: 15, delay: 8.4, duration: 22, opacity: 0.35, drift: -15 },
  { id: 2, size: 6.1, left: 23, delay: 0.5, duration: 16, opacity: 0.55, drift: 10 },
  { id: 3, size: 3.0, left: 31, delay: 14.2, duration: 26, opacity: 0.4, drift: -28 },
  { id: 4, size: 5.3, left: 38, delay: 5.7, duration: 20, opacity: 0.5, drift: 18 },
  { id: 5, size: 4.8, left: 45, delay: 11.0, duration: 24, opacity: 0.38, drift: -8 },
  { id: 6, size: 3.2, left: 52, delay: 3.3, duration: 15, opacity: 0.6, drift: 30 },
  { id: 7, size: 7.0, left: 58, delay: 17.5, duration: 28, opacity: 0.32, drift: -22 },
  { id: 8, size: 4.5, left: 64, delay: 6.8, duration: 19, opacity: 0.48, drift: 5 },
  { id: 9, size: 3.8, left: 71, delay: 1.2, duration: 21, opacity: 0.42, drift: -18 },
  { id: 10, size: 5.6, left: 77, delay: 9.6, duration: 17, opacity: 0.55, drift: 22 },
  { id: 11, size: 4.0, left: 83, delay: 15.8, duration: 25, opacity: 0.36, drift: -12 },
  { id: 12, size: 6.5, left: 90, delay: 4.0, duration: 23, opacity: 0.52, drift: 15 },
  { id: 13, size: 3.3, left: 5, delay: 12.5, duration: 27, opacity: 0.44, drift: -25 },
  { id: 14, size: 5.0, left: 35, delay: 7.2, duration: 14, opacity: 0.58, drift: 8 },
  { id: 15, size: 4.6, left: 48, delay: 18.0, duration: 20, opacity: 0.33, drift: -5 },
  { id: 16, size: 3.7, left: 62, delay: 10.3, duration: 29, opacity: 0.47, drift: 20 },
  { id: 17, size: 5.8, left: 95, delay: 0.8, duration: 16, opacity: 0.4, drift: -20 },
]

export default function FloatingParticles() {
  return (
    <div className="absolute inset-0 overflow-hidden">
      {PARTICLES.map(p => (
        <div
          key={p.id}
          className="absolute rounded-full bg-foreground/30 dark:bg-foreground/20"
          style={{
            width: p.size,
            height: p.size,
            left: `${p.left}%`,
            bottom: '-5%',
            opacity: 0,
            animation: `particle-float ${p.duration}s ${p.delay}s ease-in-out infinite`,
            ['--particle-drift' as string]: `${p.drift}px`,
            ['--particle-opacity' as string]: p.opacity,
          }}
        />
      ))}
    </div>
  )
}
