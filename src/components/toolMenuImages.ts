import { iconPaths } from './Icon';

// Render the existing toolbar style into Retina assets for the AppKit overlay.
// Keeping SVG paths and computed CSS here avoids a second platform-specific design.
export async function toolMenuImages(trigger: HTMLButtonElement, icons: (keyof typeof iconPaths)[], enabled: boolean[]) {
  const sample = trigger.cloneNode(true) as HTMLButtonElement;
  sample.removeAttribute('id');
  sample.removeAttribute('aria-controls');
  sample.setAttribute('aria-hidden', 'true');
  sample.tabIndex = -1;
  sample.querySelector('.tool-menu-corner')?.remove();
  Object.assign(sample.style, { position: 'fixed', left: '0', top: '0', visibility: 'hidden', pointerEvents: 'none' });
  document.body.append(sample);
  const scale = Math.max(2, Math.min(3, devicePixelRatio));
  const canvas = document.createElement('canvas');
  const context = canvas.getContext('2d')!;
  const prepare = (width: number, height: number) => {
    canvas.width = Math.ceil(width * scale); canvas.height = Math.ceil(height * scale);
    context.setTransform(scale, 0, 0, scale, 0, 0);
  };
  const bytes = async () => {
    const blob = await new Promise<Blob>((resolve, reject) => canvas.toBlob(value => value ? resolve(value) : reject(new Error('Cannot render tool icon'))));
    return Array.from(new Uint8Array(await blob.arrayBuffer()));
  };
  try {
    const images: number[][][] = [];
    for (const [index, tool] of icons.entries()) {
      const states: number[][] = [];
      for (const selected of [false, true]) {
        sample.className = `tool-button selection-tool-menu-item${selected ? ' selected' : ''}`;
        sample.disabled = !enabled[index];
        const svg = sample.querySelector('svg')!;
        svg.querySelector('path')!.setAttribute('d', iconPaths[tool]);
        const style = getComputedStyle(sample);
        const rect = sample.getBoundingClientRect();
        const iconRect = svg.getBoundingClientRect();
        prepare(rect.width, rect.height);
        context.globalAlpha = Number(style.opacity);
        context.fillStyle = style.backgroundColor;
        context.beginPath();
        context.roundRect(0, 0, rect.width, rect.height, parseFloat(style.borderRadius));
        context.fill();
        // Resolve currentColor before serializing the same SVG used by <Icon>.
        svg.setAttribute('stroke', style.color);
        svg.setAttribute('width', String(iconRect.width));
        svg.setAttribute('height', String(iconRect.height));
        const image = new Image();
        image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(new XMLSerializer().serializeToString(svg))}`;
        await image.decode();
        context.drawImage(image, iconRect.x - rect.x, iconRect.y - rect.y, iconRect.width, iconRect.height);
        states.push(await bytes());
      }
      images.push(states);
    }
    return { buttons: images };
  } finally { sample.remove(); }
}
