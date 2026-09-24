import type { CSSProperties, InputHTMLAttributes } from 'react';
import './compact-slider.css';

// Compact variant from the user-supplied slider reference, using app theme tokens.
export function CompactSlider({ style, min = 0, max = 100, value, ...props }: Omit<InputHTMLAttributes<HTMLInputElement>, 'type'>) {
  const progress = Math.max(0, Math.min(100, (Number(value) - Number(min)) / (Number(max) - Number(min)) * 100));
  return <input {...props} className="compact-slider" type="range" min={min} max={max} value={value} style={{
    '--slider-progress': `${progress}%`,
    '--slider-track': style?.background,
  } as CSSProperties} />;
}
