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
import {customElement} from 'lit/decorators.js';

@customElement('ns-license-info')
export class PacketInformation extends LitElement {
  static styles = css`
    .panel {
      cursor: pointer;
      display: grid;
      place-content: center;
      color: black;
      font-size: 30px;
      font-family: 'Lato', sans-serif;
      border: 5px solid black;
      border-radius: 12px;
      padding: 10px;
      background-color: #ffffff;
      max-width: max-content;
      float: left;
    }

    .title {
      font-weight: bold;
      text-align: center;
      margin-bottom: 10px;
    }

    .copyright {
      text-align: center;
      font-size: 25px;
      margin-bottom: 30px;
    }

    .licensetext {
      font-size: 20px;
      margin-bottom: 10px;
    }
  `;

  render() {
    return html`
      <div class="panel" role="text" tabindex="0">
        <div class="title">Lit-HTML</div>
        <div class="copyright">
            Copyright (c) 2017 Google LLC. All rights reserved.
        </div>
        <div class="licensetext">
            Distributed under the terms of the Lit license:<br>
            <a href="https://github.com/lit/lit/blob/main/LICENSE"
               target="_blank">https://github.com/lit/lit/blob/main/LICENSE</a>
        </div>
      </div>
    `;
  }
}
