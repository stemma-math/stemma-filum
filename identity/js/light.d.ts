export interface LightSettings {
    int: number;
    size: number;
    cy: number;
    haze: number;
    grain: number;
    speed: number;
    wob: number;
    rim: number;
}

export interface LightDrawing {
    draw(progress: number): void;
    advance(seconds: number, moving: boolean): void;
    recolour(): void;
}

export function light(canvas: HTMLCanvasElement, settings: LightSettings): LightDrawing | null;
