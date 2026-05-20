export function WorldPreview() {
  return (
    <div className="world-card" aria-label="Fishtank world preview">
      <div className="world-card-head">
        <span className="live-dot" aria-hidden />
        <strong>First Village</strong>
        <span>· public observer</span>
        <span className="tick">tick 18,402</span>
      </div>
      <div className="world-card-stage">
        <svg viewBox="0 0 320 240" xmlns="http://www.w3.org/2000/svg">
          <defs>
            <pattern id="ft-grid" width="16" height="16" patternUnits="userSpaceOnUse">
              <path
                d="M 16 0 L 0 0 0 16"
                fill="none"
                stroke="rgba(17,17,15,0.05)"
                strokeWidth="1"
              />
            </pattern>
            <linearGradient id="ft-park" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="#dfe6d8" />
              <stop offset="100%" stopColor="#cfd9c5" />
            </linearGradient>
            <linearGradient id="ft-cafe" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="#ece4d2" />
              <stop offset="100%" stopColor="#dfd5be" />
            </linearGradient>
            <linearGradient id="ft-home" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="#efebe1" />
              <stop offset="100%" stopColor="#e3ddcd" />
            </linearGradient>
          </defs>

          <rect width="320" height="240" fill="url(#ft-grid)" />

          {/* roads */}
          <path
            d="M 0 130 L 320 130"
            stroke="rgba(17,17,15,0.06)"
            strokeWidth="20"
            strokeLinecap="butt"
          />
          <path
            d="M 168 0 L 168 240"
            stroke="rgba(17,17,15,0.06)"
            strokeWidth="20"
            strokeLinecap="butt"
          />

          {/* plots */}
          <rect x="32" y="30" width="96" height="62" rx="6" fill="url(#ft-cafe)" />
          <rect x="208" y="28" width="84" height="68" rx="6" fill="url(#ft-park)" />
          <rect x="36" y="160" width="56" height="44" rx="5" fill="url(#ft-home)" />
          <rect x="104" y="160" width="56" height="44" rx="5" fill="url(#ft-home)" />
          <rect x="208" y="158" width="80" height="48" rx="6" fill="url(#ft-home)" />

          {/* tree dots on park */}
          <circle cx="225" cy="48" r="4" fill="#9fb591" />
          <circle cx="245" cy="68" r="4" fill="#9fb591" />
          <circle cx="270" cy="50" r="4" fill="#9fb591" />
          <circle cx="278" cy="78" r="4" fill="#9fb591" />

          {/* path traces */}
          <path
            d="M 96 138 Q 130 124 156 130"
            stroke="rgba(45,95,76,0.42)"
            strokeWidth="1.4"
            fill="none"
            strokeDasharray="2 3"
          />
          <path
            d="M 200 132 Q 220 110 226 96"
            stroke="rgba(176,97,64,0.5)"
            strokeWidth="1.4"
            fill="none"
            strokeDasharray="2 3"
          />

          {/* agents */}
          <g>
            <circle cx="156" cy="130" r="6.5" fill="#2d5f4c" stroke="#fff" strokeWidth="2" />
            <circle cx="226" cy="96" r="6.5" fill="#b06140" stroke="#fff" strokeWidth="2" />
            <circle cx="62" cy="178" r="6.5" fill="#3a6d8a" stroke="#fff" strokeWidth="2" />
          </g>
        </svg>

        <span className="map-tag tag-cafe">
          <span className="swatch" style={{ background: "#c79b5b" }} />
          Café Mara
        </span>
        <span className="map-tag tag-park">
          <span className="swatch" style={{ background: "#7ea36f" }} />
          Linden Park
        </span>
        <span className="map-tag tag-mia">
          <span className="swatch" style={{ background: "#2d5f4c" }} />
          Mia · walking to café
        </span>
        <span className="map-tag muted tag-otto">Otto · ordering coffee</span>
      </div>
      <div className="world-card-foot">
        <div>
          <span>Residents</span>
          <strong>12</strong>
        </div>
        <div>
          <span>Active now</span>
          <strong>3</strong>
        </div>
        <div>
          <span>Days simulated</span>
          <strong>184</strong>
        </div>
      </div>
    </div>
  );
}
