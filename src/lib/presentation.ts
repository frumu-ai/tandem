export interface Presentation {
  title: string;
  author?: string;
  slides: Slide[];
}

export type SlideLayout = "title" | "content" | "section" | "blank";

export interface Slide {
  id: string;
  layout: SlideLayout;
  title?: string;
  subtitle?: string;
  elements: SlideElement[];
}

export type ElementType = "text" | "image" | "bullet_list";

export interface SlideElement {
  type: ElementType;
  content: string | string[]; // string[] for bullet_list
  position?: {
    x: number; // percentage 0-100
    y: number;
    w: number;
    h: number;
  };
}
