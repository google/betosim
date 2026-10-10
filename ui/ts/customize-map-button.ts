// Copyright 2026 The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

import {css, html, LitElement} from 'lit';
import {customElement, property} from 'lit/decorators.js';

@customElement('ns-customize-button')
export class CustomizeButton extends LitElement {
  @property() disabled: boolean = false;

  @property() eventName: string = '';

  static styles = css`
    :host {
      display: var(--lit-button-display, inline-block);
      box-sizing: inherit;
    }

    :host(.block) {
      --lit-button-display: block;
      --lit-button-width: 100%;
    }

    .lit-button {
      background-color: var(--lit-button-bg-color, transparent);
      border: none;
      border-radius: 0.25rem;
      color: var(--lit-button-color, var(--white, #ffffff));
      cursor: pointer;
      font-weight: 400;
      font-size: 1.2rem;
      height: 2.5rem;
      line-height: 1.5;
      min-width: var(--lit-button-min-width, 10rem);
      outline: 0;
      padding: 0 var(--lit-button-padding-horizontal, 1rem);
      position: relative;
      transition: background-color 0.15s ease-in-out 0s;
      text-align: center;
      text-decoration: none;
      text-transform: none;
      user-select: none;
      vertical-align: middle;
      width: var(--lit-button-width, auto);
    }
    .lit-button-icon {
      --lit-button-min-width: 5rem;
      --lit-button-padding-horizontal: 0;
    }

    button[disabled],
    button[disabled]:hover {
      opacity: 0.65;
      pointer-events: none;
    }

    button:focus::before {
      content: '';
      border-radius: 0.25rem;
      border: 1px solid var(--white, #fff);
      box-sizing: inherit;
      display: block;
      position: absolute;
      height: calc(100% - 0.8rem);
      top: 0.4rem;
      left: 0.4rem;
      visibility: visible;
      width: calc(100% - 0.8rem);
    }

    :host(.primary) {
      --lit-button-bg-color: var(--primary, #903d57);
    }

    :host(.primary) button:active,
    :host(.primary) button:hover {
      --lit-button-bg-color: var(--primary-active, #0062cc);
    }

    :host(.icon) {
      --lit-button-min-width: 5rem;
      --lit-button-padding-horizontal: 0;
    }
  `;

  render() {
    return html`
      <button
        @click="${() => {
      window.dispatchEvent(new CustomEvent(this.eventName));
    }}"
        class="lit-button"
        ?disabled=${this.disabled}
      >
        <slot></slot>
      </button>
    `;
  }
}
