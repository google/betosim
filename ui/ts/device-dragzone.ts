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

import {html, LitElement} from 'lit';
import {customElement, property} from 'lit/decorators.js';

import {simulationState} from './device-observer.js';

@customElement('ns-device-dragzone')
export class DeviceDragZone extends LitElement {
  static dragged: EventTarget|null;

  @property({type: String, attribute: 'action'}) action = 'move';

  constructor() {
    super();
    this.addEventListener('dragstart', this.handleDragStart);
    this.addEventListener('dragend', this.handleDragEnd);
    this.addEventListener('click', this.handleSelect);
  }

  connectedCallback() {
    this.draggable = true;
  }

  handleDragStart(ev: DragEvent) {
    this.style.opacity = '0.4';
    if (ev.dataTransfer && ev.target) {
      simulationState.patchSelected((ev.target as Element).id);
      DeviceDragZone.dragged = ev.target;
      // eslint-disable-next-line no-param-reassign
      ev.dataTransfer.effectAllowed = this.action === 'move' ? 'move' : 'copy';
    }
  }

  handleDragEnd() {
    this.style.opacity = '1';
    DeviceDragZone.dragged = null;
  }

  // Allow the information panel to figure what has been selected.
  handleSelect(ev: Event) {
    this.style.opacity = '1';
    if (ev.target) {
      simulationState.patchSelected((ev.target as Element).id);
      // We can add a feature for visually showing a selected object (i.e.
      // bolded borders)
    }
  }

  render() {
    return html` <slot></slot> `;
  }
}
