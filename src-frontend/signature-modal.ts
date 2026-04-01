type SignatureReadyHandler = (imageData: string) => void;

export class SignatureModal {
  private backdrop: HTMLElement;
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;

  private isDrawing = false;
  private hasContent = false;

  private handlers: SignatureReadyHandler[] = [];

  constructor() {
    this.backdrop = document.getElementById("signature-modal")!;
    this.canvas = document.getElementById("signature-canvas") as HTMLCanvasElement;
    this.ctx = this.canvas.getContext("2d")!;

    this.initDrawing();
    this.bindButtons();
  }

  onSignatureReady(handler: SignatureReadyHandler): void {
    this.handlers.push(handler);
  }

  private emit(imageData: string): void {
    this.handlers.forEach((h) => h(imageData));
  }

  open(): void {
    this.clearCanvas();
    this.backdrop.classList.remove("hidden");
    this.canvas.focus();
  }

  close(): void {
    this.backdrop.classList.add("hidden");
  }

  private clearCanvas(): void {
    this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
    this.hasContent = false;
  }

  private initDrawing(): void {
    this.canvas.style.touchAction = "none";

    this.canvas.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      this.isDrawing = true;
      this.canvas.setPointerCapture(e.pointerId);
      this.ctx.beginPath();
      this.ctx.moveTo(e.offsetX, e.offsetY);

      this.ctx.strokeStyle = "#000";
      this.ctx.lineWidth = Math.max(1, (e.pressure || 0.5) * 3);
      this.ctx.lineCap = "round";
      this.ctx.lineJoin = "round";
    });

    this.canvas.addEventListener("pointermove", (e) => {
      if (!this.isDrawing) return;
      e.preventDefault();
      this.ctx.lineWidth = Math.max(1, (e.pressure || 0.5) * 3);
      this.ctx.lineTo(e.offsetX, e.offsetY);
      this.ctx.stroke();
      this.ctx.beginPath();
      this.ctx.moveTo(e.offsetX, e.offsetY);
      this.hasContent = true;
    });

    const stopDrawing = (): void => {
      this.isDrawing = false;
    };
    this.canvas.addEventListener("pointerup", stopDrawing);
    this.canvas.addEventListener("pointercancel", stopDrawing);
  }

  private bindButtons(): void {
    document
      .getElementById("sig-close-btn")!
      .addEventListener("click", () => this.close());

    document.getElementById("sig-clear-btn")!.addEventListener("click", () => {
      this.clearCanvas();
    });

    document.getElementById("sig-place-btn")!.addEventListener("click", () => {
      if (!this.hasContent) return;
      const b64 = this.canvas
        .toDataURL("image/png")
        .split(",")[1]; // strip data:image/png;base64,
      this.close();
      this.emit(b64);
    });

    document
      .getElementById("sig-image-btn")!
      .addEventListener("click", () => this.loadImageFile());
  }

  private loadImageFile(): void {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/png,image/jpeg";
    input.style.display = "none";

    input.addEventListener("change", () => {
      const file = input.files?.[0];
      if (!file) return;

      const reader = new FileReader();
      reader.onload = (e) => {
        const dataUrl = e.target?.result as string;
        if (!dataUrl) return;

        const img = new Image();
        img.onload = () => {
          this.clearCanvas();
          // Scale to fit the canvas while preserving aspect ratio
          const scaleX = this.canvas.width / img.width;
          const scaleY = this.canvas.height / img.height;
          const s = Math.min(scaleX, scaleY, 1);
          const dx = (this.canvas.width - img.width * s) / 2;
          const dy = (this.canvas.height - img.height * s) / 2;
          this.ctx.drawImage(img, dx, dy, img.width * s, img.height * s);
          this.hasContent = true;
        };
        img.src = dataUrl;
      };
      reader.readAsDataURL(file);
      input.remove();
    });

    document.body.appendChild(input);
    input.click();
  }
}
