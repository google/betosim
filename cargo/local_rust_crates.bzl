# Copyright 2026 The Android Open Source Project
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""Defines a module extension to create repos for local rust crates for netsim."""

load("@bazel_tools//tools/build_defs/repo:git.bzl", "git_repository")
load("@bazel_tools//tools/build_defs/repo:http.bzl", "http_archive")
load("@bazel_tools//tools/build_defs/repo:local.bzl", "local_repository", "new_local_repository")
load("//cargo:repository_rules.bzl", "patched_new_local_repository")

_CRATES = {
    "aho-corasick": ("1.1.5", "c982642fa9e8606056828ee9a8505737230110bb1099153c79efe865c59d12ba"),
    "android_log-sys": ("0.3.2", "84521a3cf562bc62942e294181d9eef17eb38ceb8c68677bc49f144e4c3d4f8d"),
    "anstyle": ("1.0.14", "940b3a0ca603d1eade50a4846a2afffd5ef57a9feac2c0e2ec2e14f9ead76000"),
    "anyhow": ("1.0.104", "330a5ed07fa54e4702c9d6c4174f74427fc0ef6e214bbd677ae50a5099946470"),
    "argh": ("0.1.19", "211818e820cda9ca6f167a64a5c808837366a6dfd807157c64c1304c486cd033"),
    "argh_derive": ("0.1.19", "c442a9d18cef5dde467405d27d461d080d68972d6d0dfd0408265b6749ec427d"),
    "argh_shared": ("0.1.19", "e5ade012bac4db278517a0132c8c10c6427025868dca16c801087c28d5a411f1"),
    "arrayvec": ("0.7.8", "d3fb67a6e08acf24fdeccbac2cb6ac4305825bd1f117462e0e6f2f193345ad56"),
    "async-trait": ("0.1.89", "9035ad2d096bed7955a320ee7e2230574d28fd3c3a0f186cbea1ff3c7eed5dbb"),
    "atomic-waker": ("1.1.2", "1505bd5d3d116872e7271a6d4e16d81d0c8570876c8de68093a09ac269d8aac0"),
    "base64": ("0.23.1", "ac07cdecf99051d9a5238b80f35af32cdeba5b336e55d957b318b50137e18da5"),
    "bitflags": ("2.13.2", "3ded4057c258ba199e2d26386d3af3780957ecaee6c4ef4041c6b4b8b97c0b06"),
    "byteorder": ("1.5.0", "1fd0f2584146f6f2ef48085050886acf353beff7305ebd1ae69500e27c67f64b"),
    "bytes": ("1.12.1", "fc652a48c352aef3ea3aed32080501cf3ef6ed5da78602a020c991775b0aff04"),
    "cfg-if": ("1.0.5", "4e7648175b45a9a48536d676f68d918270699102aa8dab5496df06904c914600"),
    "chacha20": ("0.10.2", "65c35e4b699c7e15ccbe7ee35c005e4fc0a278d22238a2857e6ce2dadeda1b06"),
    "chrono": ("0.4.45", "1aa79e62e7697b8e29b513a68abacf485adcd1fe8284a4316c5ae868e6633327"),
    "clap": ("4.6.1", "1ddb117e43bbf7dacf0a4190fef4d345b9bad68dfc649cb349e7d17d28428e51"),
    "clap_builder": ("4.6.0", "714a53001bf66416adb0e2ef5ac857140e7dc3a0c48fb28b2f10762fc4b5069f"),
    "clap_derive": ("4.6.1", "f2ce8604710f6733aa641a2b3731eaa1e8b3d9973d5e3565da11800813f997a9"),
    "clap_lex": ("1.1.1", "1c133bc6a41be0d194c306b5506d15e6feeea7b1d6604bd3f8310dfb2ca96486"),
    "codespan-reporting": ("0.13.1", "af491d569909a7e4dee0ad7db7f5341fef5c614d5b8ec8cf765732aba3cff681"),
    "command-fds": ("0.3.3", "1b60b5124979fccd9addd89d8b97a1d6eebb4950694520c75ddd722535ea443f"),
    "cpufeatures": ("0.3.1", "5ca28b0ae3115b884660db4118d803791fd6756b6e88f39c0f3f7859060d7566"),
    "crc32fast": ("1.5.2", "01a7799fd6b852db0e61728dde9a204c423b44d689dbd432522543614b490e78"),
    "data-encoding": ("2.11.1", "4583a4551df46e2792f82ceeac45e850d2e2d5debba0b91f102385cda5b11f06"),
    "displaydoc": ("0.2.6", "1ac70aa55017e108007fbaf5aa0f54b021c98f92ff8af59d42eda9da96e3dd4f"),
    "downcast": ("0.11.0", "1435fa1053d8b2fbbe9be7e97eca7f33d37b28409959813daefc1446a14247f1"),
    "env_filter": ("2.0.0", "900d271a03799a1ee8d1ca9b19893b48ca674a9284fefcfb85f05e74ed314217"),
    "env_logger": ("0.11.11", "de671bd27a75a797dc9ae289ba1e77276e75e2026408aab65185384e2d5cd3f6"),
    "equivalent": ("1.0.2", "877a4ace8713b0bcf2a4e7eec82529c029f1d0619886d18145fea96c3ffe5c0f"),
    "errno": ("0.3.14", "39cab71617ae0d63f51a36d69f866391735b51691dbda63cf6f96d042b63efeb"),
    "fastrand": ("2.5.0", "da7c62ceae207dd37ea5b845da6a0696c799f85e97da1ab5b7910be3c1c80223"),
    "flate2": ("1.1.9", "843fba2746e448b37e26a819579957415c8cef339bf08564fe8b7ddbd959573c"),
    "fnv": ("1.0.7", "3f9eec918d3f24069decb9af1554cad7c880e2da24a9afd88aca000531ab82c1"),
    "foldhash": ("0.2.0", "77ce24cb58228fbb8aa041425bb1050850ac19177686ea6e0f41a70416f56fdb"),
    "form_urlencoded": ("1.2.2", "cb4cb245038516f5f85277875cdaa4f7d2c9a0fa0468de06ed190163b1581fcf"),
    "fragile": ("2.1.0", "8878864ba14bb86e818a412bfd6f18f9eabd4ec0f008a28e8f7eb61db532fcf9"),
    "futures": ("0.3.34", "9a31d2a3fbaaeb2af2368bbdd904aa8e812d3c04a1ee10d3171f52d556e5d0a3"),
    "futures-channel": ("0.3.34", "b1f9e3d69d39e4862ffed03ed071a76f9a13ba1d9109d355b0f0aa6b15e393c4"),
    "futures-core": ("0.3.34", "92d699e522242e69e3003b94ecc1f960f3a5e015aa7c5d7486e65ad01dd94f5e"),
    "futures-executor": ("0.3.34", "031b47cf1a3c6cc8bc2fc76cd437f521619387907d469316e7c0bc278f1f5432"),
    "futures-io": ("0.3.34", "53c0fa8157de1303bfffdaa1cc2a673bfffb60102f76b0ef4441659124373fed"),
    "futures-macro": ("0.3.34", "9fb9654ba8355388abeb8dcb4fc62f511300867002afc858860463bdd9fe0c44"),
    "futures-sink": ("0.3.34", "1944426bf7d03f1d14f708785e4b33efd750b36d48a157b836b3efc15ede8e1d"),
    "futures-task": ("0.3.34", "cd417de3d1d015fc3bfd2b1ea46dfc7bab72ef86f1cc7cc9c78e728b34a6d1fd"),
    "futures-util": ("0.3.34", "0d50a92467f8ba5dd6e3ee5d4bd04d73ab2e4e1c44474a0674821dfce14b79bc"),
    "getrandom": ("0.4.3", "300e883d756b2e4ec94e02791f39b04b522276138852cfc41d9fb7e904106099"),
    "getrandom_v0_2": ("0.2.12", "190092ea657667030ac6a35e305e62fc4dd69fd98ac98631e5d3a2b1575a12b5"),
    "gherkin": ("0.16.0", "9e2c0d8c632f8a251ce9a8198079b1022adc586ff4e3d33e18debd40eb463b31"),
    "glam": ("0.33.6", "21fef0953c54fd3de2f44b743fbf77e044c81a25faee03636dfccc0d35135e23"),
    "grpcio": ("0.13.0", "9e398946b5721d72478eb647260a1b7c1d5f70f0de35399846c3913bd369a33e"),
    "grpcio-sys": ("0.13.0+1.56.2-patched", "b3dae9132320ae1b03ea55b5ddc88ca72a31fb85fa631a241a40157f5feffe43"),
    "h2": ("0.4.19", "ef8e5e5a340588f4452631496976cf8636d4a7ecf600239fdc27615d2530bc16"),
    "hashbrown": ("0.17.1", "ed5909b6e89a2db4456e54cd5f673791d7eca6732202bbf2a9cc504fe2f9b84a"),
    "heck": ("0.5.0", "2304e00983f87ffb38b55b444b5e3b60a884b5d30c0fca7d82fe33449bbe55ea"),
    "hex": ("0.4.3", "7f24254aa9a54b5c858eaee2f5bccdb46aaf0e486a595ed5fd8f86ba55232a70"),
    "http": ("1.5.0", "918d3568bebf352712bc2ef3d46a8bcf1a75b373be6539de198e9105cbbf9ce0"),
    "http-body": ("1.1.0", "ca2a8f2913ee65f60facd6a5905613afaa448497a0230cc41ce022d93290bc2c"),
    "http-body-util": ("0.1.5", "23169fe34a5fbcdd3f3862e78fb9b6fccd5f02a6dc6f732547005d45631ce71c"),
    "httparse": ("1.10.1", "6dbf3de79e51f3d586ab4cb9d5c3e2c14aa28ed23d180cf89b4df0454a69cc87"),
    "httpdate": ("1.0.3", "df3b46402a9d5adb4c86a0cf463f42e19994e3ee891101b1841f30a545cb49a9"),
    "hyper": ("1.11.1", "27b501faa50e7a26c3d3560ca625132f4078a17771f4810baf70475ae48cbe43"),
    "hyper-util": ("0.1.21", "ddc03d96684f9226b8a787cdb71488417b53ab5ea8fdb1dac946cb9431cc8bff"),
    "icu_collections": ("2.2.0", "2984d1cd16c883d7935b9e07e44071dca8d917fd52ecc02c04d5fa0b5a3f191c"),
    "icu_locale_core": ("2.2.0", "92219b62b3e2b4d88ac5119f8904c10f8f61bf7e95b640d25ba3075e6cac2c29"),
    "icu_normalizer": ("2.2.0", "c56e5ee99d6e3d33bd91c5d85458b6005a22140021cc324cea84dd0e72cff3b4"),
    "icu_normalizer_data": ("2.2.0", "da3be0ae77ea334f4da67c12f149704f19f81d1adf7c51cf482943e84a2bad38"),
    "icu_properties": ("2.2.0", "bee3b67d0ea5c2cca5003417989af8996f8604e34fb9ddf96208a033901e70de"),
    "icu_properties_data": ("2.2.0", "8e2bbb201e0c04f7b4b3e14382af113e17ba4f63e2c9d2ee626b720cbce54a14"),
    "icu_provider": ("2.2.0", "139c4cf31c8b5f33d7e199446eff9c1e02decfc2f0eec2c8d71f65befa45b421"),
    "idna": ("1.1.0", "3b0875f23caa03898994f6ddc501886a45c7d3d62d04d2d90788d47be1b1e4de"),
    "idna_adapter": ("1.2.2", "cb68373c0d6620ef8105e855e7745e18b0d00d3bdb07fb532e434244cdb9a714"),
    "indexmap": ("2.14.2", "cc4e190f5d26ca7051642629da2c52fc03bde85a03197c99408dcd291734c855"),
    "ipnet": ("2.12.2", "791930b43c0d5973160d90a8f3894509f2b273430f5c5c73b668636d0287c5c0"),
    "itoa": ("1.0.18", "8f42a60cbdf9a97f5d2305f08a87dc4e09308d1276d28c869c684d7777685682"),
    "jiff": ("0.2.32", "961d16382652bfdd8c6f68b223b26a8c93e0d475c672f414411db31c6c5c900e"),
    "lazy_static": ("1.5.0", "bbd2bcb4c963f2ddae06a2efc7e9f3591312473c50c6685e1f298068316e66fe"),
    "libc": ("0.2.189", "3eaf3ede3fee6db1a4c2ee091bf8a8b4dccdc6d17f656fb07896ee72867612f2"),
    "libz-sys": ("1.1.29", "85bc9657773828b90eeb625adff10eeac83cc21bbfd8e23a03eaa8a33c9e28d9"),
    "litemap": ("0.8.3", "47d9d19d1d6efa0109d2f65ff4c85cddd50bd572e5a00127ab10987290bcefae"),
    "lock_api": ("0.4.14", "224399e74b87b5f3557511d98dff8b14089b3dadafcab6bb93eab67d3aace965"),
    "log": ("0.4.34", "f9f8bd3e56ce4dfc153cf470fffbfa98c7620958b312ca5c3a4b8d5181fd13c6"),
    "matchers": ("0.2.0", "d1525a2a28c7f4fa0fc98bb91ae755d1e2d1505079e05539e35bc876b5d65ae9"),
    "memchr": ("2.8.3", "cf8baf1c55e62ffcace7a9f06f4bd9cd3f0c4beb022d3b367256b91b87513d98"),
    "memoffset": ("0.9.1", "488016bfae457b036d996092f6cb448677611ce4449e970ceaf42695203f218a"),
    "minimal-lexical": ("0.2.1", "68354c5c6bd36d73ff3feceb05efa59b6acb7626617f4962be322a825e61f79a"),
    "mio": ("1.2.3", "4b18443e9c262bfe8fa82f51666e2642c53393f7e5c27b3e1aeab922cff5b9d8"),
    "mockall": ("0.15.0", "1a6ceddfe3ce334925e96bf420fdb2dcee5bed6c632a168ece622676dadeaf8a"),
    "mockall_derive": ("0.15.0", "9cfe16fbe8a314aeec0b861ac24e60b1e123e97634bab045475b9d6a18416fd8"),
    "nix": ("0.31.3", "cf20d2fde8ff38632c426f1165ed7436270b44f199fc55284c38276f9db47c3d"),
    "nom": ("7.1.3", "d273983c5a657a70a3e8f2a01329822f3b8c8172b73826411a55751e404a0a4a"),
    "num-bigint": ("0.5.1", "93e7820bc0a80a0238e650327316f929ba18d5be054b647490a3a6a339f3e7c0"),
    "num-derive": ("0.5.1", "e4e98dc3b890f6c23a0f9d3d491a2823d0dea0fa656302a13dd225fa924112a8"),
    "num-integer": ("0.1.47", "7ce2d95d4b3734dc35aa2f45e1aa22cd416814592a4f9d9205e11affd5b8e10b"),
    "num-traits": ("0.2.19", "071dfc062690e90b734c0b2273ce72ad0ffa95f0c74596bc250dcfd960262841"),
    "num_cpus": ("1.17.0", "91df4bbde75afed763b708b7eee1e8e7651e02d97f6d5dd763e89367e957b23b"),
    "once_cell": ("1.21.4", "9f7c3e4beb33f85d45ae3e3a1792185706c8e16d043238c593331cc7cd313b50"),
    "parking_lot": ("0.12.5", "93857453250e3077bd71ff98b6a65ea6621a19bb0f559a85248955ac12c45a1a"),
    "parking_lot_core": ("0.9.12", "2621685985a2ebf1c516881c026032ac7deafcda1a2c9b7850dc81e3dfcb64c1"),
    "pdl-compiler": ("0.5.2", "a9eef2e2c3cc7af6282f40085b33e37ae48f0ea23082bffdb01ed061df12b0a4"),
    "pdl-runtime": ("0.5.2", "5ba295ad92bae1d85d1bd85853c51e7ea10a0eb8ffd30948822d1a7b26ed0666"),
    "peg": ("0.8.6", "0aad070be5b63aa72103f2fcdd70a83adbd5e90112ce5b574171ff1c65501773"),
    "peg-macros": ("0.8.6", "ddd8ef6825cae95355031ae26a99b616a2a21f22ba2de0197c43dfb05acbe7ee"),
    "peg-runtime": ("0.8.6", "7011d97b484a5ebdc4b1fdb3b12d5e4bbbea56e9d22b688f2e79e04b65a7d8a6"),
    "percent-encoding": ("2.3.2", "9b4f627cb1b25917193a259e49bdad08f671f8d9708acfd5fe0a8c1455d87220"),
    "pest": ("2.9.2", "45d3aca230fad2e6f6317ca0a72724338c4960cb97168a85cdee66df4a9a21a8"),
    "pest_derive": ("2.9.0", "b3a83744a5c8455b8b3e0dc5031362780a347c878bdd11584d1a8984228cc88d"),
    "pest_generator": ("2.9.0", "e0cd3451aa3de60d4b9a1e736885e4dea6b31617598026f12256ad566d63304a"),
    "pest_meta": ("2.9.0", "e04d3a0849e241d7dfce834c83b1c5edc8622009e8dd51a12ba1927c32f05496"),
    "pica": ("0.1.13", "b88106b369c5e43802f2d9830266405ff352902543054b9535d998ac17b1e2cf"),
    "pin-project-lite": ("0.2.17", "a89322df9ebe1c1578d689c92318e070967d1042b512afbe49518723f4e6d5cd"),
    "pin-utils": ("0.1.0", "8b870d8c151b6f2fb93e84a13146138f05d02ed11c7e7c54f8826aaaf7c9f184"),
    "potential_utf": ("0.1.6", "d83eb9bc6d8e5cf568e7a1101d60ee05e81ed50ea106026f3d18deeb046d7661"),
    "predicates": ("3.1.4", "ada8f2932f28a27ee7b70dd6c1c39ea0675c55a36879ab92f3a715eaa1e63cfe"),
    "predicates-core": ("1.0.10", "cad38746f3166b4031b1a0d39ad9f954dd291e7854fcc0eed52ee41a0b50d144"),
    "predicates-tree": ("1.0.13", "d0de1b847b39c8131db0467e9df1ff60e6d0562ab8e9a16e568ad0fdb372e2f2"),
    "prettyplease": ("0.2.37", "479ca8adacdd7ce8f1fb39ce9ecccbfe93a3f1344b3d0d97f20bc0196208f62b"),
    "proc-macro2": ("1.0.107", "985e7ec9bb745e6ce6535b544d84d6cd6f7ad8bd711c398938ae983b91a766d9"),
    "protobuf-codegen": ("3.7.2", "5d3976825c0014bbd2f3b34f0001876604fe87e0c86cd8fa54251530f1544ace"),
    "protobuf-json-mapping": ("3.7.2", "e0d6e4be637b310d8a5c02fa195243328e2d97fa7df1127a27281ef1187fcb1d"),
    "protobuf-parse": ("3.7.2", "b4aeaa1f2460f1d348eeaeed86aea999ce98c1bded6f089ff8514c9d9dbdc973"),
    "protobuf-rust": ("3.7.2", "d65a1d4ddae7d8b5de68153b48f6aa3bba8cb002b243dbdbc55a5afbc98f99f4"),
    "protobuf-support": ("3.7.2", "3e36c2f31e0a47f9280fb347ef5e461ffcd2c52dd520d8e216b52f93b0b0d7d6"),
    "quote": ("1.0.47", "1fbf4db142a473a8d80c26bbf18454ed458bf8d26c8219c331daecfdbd079001"),
    "rand": ("0.10.2", "c7f5fa3a058cd35567ef9bfa5e75732bee0f9e4c55fa90477bef2dfcdbc4be80"),
    "rand_core": ("0.10.1", "63b8176103e19a2643978565ca18b50549f6101881c443590420e4dc998a3c69"),
    "regex": ("1.13.1", "f020237b6c8eed93db2e2cb53c00c60a8e1bc73da7d073199a1180401450218d"),
    "regex-automata": ("0.4.18", "ad8553b9b26413251cbf30e620595c7a41b3887f03da04579c0e6b0d6a06b4b2"),
    "regex-syntax": ("0.8.11", "d6f6ff9a378485b298a5286656da665ba74413d36db0979633275d2e708145d4"),
    "ring": ("0.17.2", "911b295d2d302948838c8ac142da1ee09fa7863163b44e6715bc9357905878b8"),
    "rustix": ("1.1.4", "b6fe4565b9518b83ef4f91bb47ce29620ca828bd32cb7e408f0062e9930ba190"),
    "ryu": ("1.0.23", "9774ba4a74de5f7b1c1451ed6cd5285a32eddb5cccb8cc655a4e50009e06477f"),
    "scopeguard": ("1.2.0", "94143f37725109f92c262ed2cf5e59bce7498c01bcc1502d7b9afe439a4e9f49"),
    "serde": ("1.0.228", "9a8e94ea7f378bd32cbbd37198a4a91436180c5bb472411e48b5ec2e2124ae9e"),
    "serde-xml-rs": ("0.6.0", "fb3aa78ecda1ebc9ec9847d5d3aba7d618823446a049ba2491940506da6e2782"),
    "serde_core": ("1.0.228", "41d385c7d4ca58e59fc732af25c3983b67ac852c1a25000afe1175de458b67ad"),
    "serde_derive": ("1.0.228", "d540f220d3187173da220f885ab66608367b6574e925011a9353e4badda91d79"),
    "serde_json": ("1.0.151", "c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14"),
    "serde_yaml": ("0.9.34+deprecated", "6a8b1a1a2ebf674015cc02edccce75287f1a0130d394307b36743c2f5d504b47"),
    "sharded-slab": ("0.1.7", "f40ca3c46823713e0d4209592e8d6e826aa57e928f09752619fc696c499637f6"),
    "signal-hook-registry": ("1.4.8", "c4db69cba1110affc0e9f7bcd48bbf87b3f4fc7c61fc9155afd4c469eb3d6c1b"),
    "slab": ("0.4.12", "0c790de23124f9ab44544d7ac05d60440adc586479ce501c1d6d7da3cd8c9cf5"),
    "smallvec": ("1.16.0", "b9be42f50aa861c555654aa3a37f52f4b1074bacf4e48fe0ef7fa584e80f1f0f"),
    "socket2": ("0.6.5", "c3d1e2c7f27f8d4cb10542a02c49005dbd6e93095799d6f3be745fae9f8fedd4"),
    "spin": ("0.12.3", "0134f9043ed38b087ac4f7d4af44c79e2c9e5094421fe3164f435ce585953b10"),
    "stable_deref_trait": ("1.2.1", "6ce2be8dc25455e1f91df71bfa12ad37d7af1092ae736f3a6cd0e37bc7810596"),
    "strsim": ("0.11.1", "7da8b5736845d9f2fcb837ea5d9e2628564b3b043a70948a3f0b778838c5fb4f"),
    "syn": ("2.0.119", "872831b642d1a07999a962a351ed35b955ea2cfc8f3862091e2a240a84f17297"),
    "synstructure": ("0.13.2", "728a70f3dbaf5bab7f0c4b1ac8d7ae5ea60a4b5549c8a5914361c99147a709d2"),
    "tempfile": ("3.27.0", "32497e9a4c7b38532efcdebeef879707aa9f794296a4f0244f6f69e9bc8574bd"),
    "termcolor": ("1.4.1", "06794f8f6c5c898b3275aebefa6b8a1cb24cd2c6c79397ab15774837a0bc5755"),
    "termtree": ("1.0.0", "d4d1330fe7f7f872cd05165130b10602d667b205fd85be09be2814b115d4ced9"),
    "textwrap": ("0.16.2", "c13547615a44dc9c452a8a534638acdf07120d4b6847c8178705da06306a3057"),
    "thiserror": ("2.0.18", "4288b5bcbc7920c07a1149a35cf9590a2aa808e0bc1eafaade0b80947865fbc4"),
    "thiserror-impl": ("2.0.18", "ebc4ee7f67670e9b64d05fa4253e753e016c6c95ff35b89b7941d6b856dec1d5"),
    "thread_local": ("1.1.10", "1ad99c4c6d32803332c548b1af0540b357b3f5fc0be8f6c6bfe8b2e6ae784070"),
    "tinystr": ("0.8.4", "b1e27c91459209c2986af3dcf603a5a74a4368754ce37414f59acc971167f643"),
    "tokio": ("1.53.1", "202caea871b69668250d242070849eb495be178ed697a3e98aebce5bc81a0bed"),
    "tokio-macros": ("2.7.1", "6328af13490e73a9b4694030fafd93f8c8c6a9dede33e821c3fc63eddf8042ba"),
    "tokio-stream": ("0.1.19", "a3d06f0b082ba57c26b79407372e57cf2a1e28124f78e9479fe80322cf53420b"),
    "tokio-util": ("0.7.19", "494815d09bf52b5548659851081238f0ca39ff638363907596da739561c62c52"),
    "tower-layer": ("0.3.3", "121c2a6cda46980bb0fcd1647ffaf6cd3fc79a013de288782836f6df9c48780e"),
    "tower-service": ("0.3.3", "8df9b6e13f2d32c91b9bd719c00d1958837bc7dec474d94952798cc8e69eeec3"),
    "tracing": ("0.1.44", "63e71662fa4b2a2c3a26f570f037eb95bb1f85397f3cd8076caed2f026a6d100"),
    "tracing-attributes": ("0.1.31", "7490cfa5ec963746568740651ac6781f701c9c5ea257c58e057f3ba8cf69e8da"),
    "tracing-core": ("0.1.36", "db97caf9d906fbde555dd62fa95ddba9eecfd14cb388e4f491a66d74cd5fb79a"),
    "tracing-log": ("0.2.0", "ee855f1f400bd0e5c02d150ae5de3840039a3f54b025156404e34c23c03f47c3"),
    "tracing-subscriber": ("0.3.23", "cb7f578e5945fb242538965c2d0b04418d38ec25c79d160cd279bf0731c8d319"),
    "try-lock": ("0.2.5", "e421abadd41a4225275504ea4d6566923418b7f05506fbc9c0fe86ba7396114b"),
    "tungstenite": ("0.30.0", "e48ac77174b19c110a50ab2128b24215ac9cb40e0e12e093fb602d175c569d22"),
    "typed-builder": ("0.23.2", "31aa81521b70f94402501d848ccc0ecaa8f93c8eb6999eb9747e72287757ffda"),
    "typed-builder-macro": ("0.23.2", "076a02dc54dd46795c2e9c8282ed40bcfb1e22747e955de9389a1de28190fb26"),
    "ucd-trie": ("0.1.7", "2896d95c02a80c6d6a5d6e953d479f5ddf2dfdb6a244441010e373ac0fb88971"),
    "unicode-bidi": ("0.3.18", "5c1cb5db39152898a79168971543b1cb5020dff7fe43c8dc468b0885f5e29df5"),
    "unicode-ident": ("1.0.25", "ab72a15cf68d77cb0987d3684aa8a45c5ef827e8cb49ee2f30bfd7ba2feb519f"),
    "unicode-width": ("0.2.2", "b4ac048d71ede7ee76d585517add45da530660ef4390e49b098733c6e897f254"),
    "unsafe-libyaml": ("0.2.11", "673aac59facbab8a9007c7f6108d11f63b603f7cabff99fabf650fea5c32b861"),
    "untrusted": ("0.9.0", "8ecb6da28b8a351d773b68d5825ac39017e680750f980f3a1a85cd8dd28a47c1"),
    "url": ("2.5.8", "ff67a8a4397373c3ef660812acab3268222035010ab8680ec4215f38ba3d0eed"),
    "utf-8": ("0.7.6", "09cc8ee72d2a9becf2f2febe0205bbed8fc6615b7cb429ad062dc7b7ddd036a9"),
    "utf8_iter": ("1.0.4", "b6c140620e7ffbb22c2dee59cafe6084a59b5ffc27a8859a5f0d494b5d52b6be"),
    "want": ("0.3.1", "bfa7760aed19e106de2c7c0b581b509f2f25d3dacaf737cb82ac61bc6d760b0e"),
    "which": ("8.0.6", "bae2f2b2b816647a1cab1acc91f5bd20812d53cb344382635ec2181940c8034f"),
    "winapi-util": ("0.1.11", "c2a7b1c03c876122aa43f3020e6c3c3ee5c05081c9a00739faf7503aeba10d22"),
    "windows-link": ("0.2.1", "f0805222e57f7521d6a62e36fa9163bc891acd422f971defe97d64e70d0a4fe5"),
    "windows-sys": ("0.61.2", "ae137229bcbd6cdf0f7b80a31df61766145077ddf49416a728b02cb3921ff3fc"),
    "windows-targets": ("0.53.5", "4945f9f551b88e0d65f3db0bc25c33b8acea4d9e41163edf90dcd0b19f9069f3"),
    "windows_aarch64_gnullvm": ("0.53.1", "a9d8416fa8b42f5c947f8482c43e7d89e73a173cead56d044f6a56104a6d1b53"),
    "windows_i686_gnullvm": ("0.53.1", "fa7359d10048f68ab8b09fa71c3daccfb0e9b559aed648a8f95469c27057180c"),
    "windows_x86_64_gnullvm": ("0.53.1", "0ffa179e2d07eee8ad8f57493436566c7cc30ac536a3379fdf008f47f6bb7ae1"),
    "writeable": ("0.6.4", "3ad82d2a33cdc9674dc7465672f271e096168fcdbe0f799d9e6db8c5892679dc"),
    "xml-rs": ("0.8.20", "791978798f0597cfc70478424c2b4fdc2b7a8024aaff78497ef00f24ef674193"),
    "yoke": ("0.8.3", "709fe23a0424b6a435d82152b1bd3fdfb0833487d5fa90d05d42762a9891fef5"),
    "yoke-derive": ("0.8.2", "de844c262c8848816172cef550288e7dc6c7b7814b4ee56b3e1553f275f1858e"),
    "zerocopy": ("0.8.57", "d35102a9f36d089ccae9e4c6802bc118be4487b80aaffc0ab4e0cf5ce92d2873"),
    "zerocopy-derive": ("0.8.57", "146c01f5ab44258da43cf276c74a2763db2ff3969c9c652c3f2de07041d0b2bc"),
    "zerofrom": ("0.1.8", "0ec05a11813ea801ff6d75110ad09cd0824ddba17dfe17128ea0d5f68e6c5272"),
    "zerofrom-derive": ("0.1.7", "11532158c46691caf0f2593ea8358fed6bbf68a0315e80aae9bd41fbade684a1"),
    "zerotrie": ("0.2.5", "4ea269c3bd32f0a32c321907a2ae912ba6f4649bb0fc764a15627e99a7095a3f"),
    "zerovec": ("0.11.8", "bb0464e17806c1d976d5cba29399c7f08e516e279e2ba493f63123b5fca67dd8"),
    "zerovec-derive": ("0.11.6", "34df6fc39dbd26ddc9c10e6a2984476e13acce22e64e4487636ef494369225da"),
    "zip": ("0.6.6", "760394e246e4c28189f19d488c058bf16f564016aefac5d32bb1f3b51d5e9261"),
    "zlib-rs": ("0.6.7", "34b31d188d9d685a4f9c7b46d6e36631b07058d2cfe190267adce54dc230bf12"),
    "zmij": ("1.0.23", "29666d0abbfad1e3dc4dcf6144730dd3a3ab225bbbdac83319345b1b44ccfc1b"),
}

_CRATE_NAMES = {
    "getrandom_v0_2": "getrandom",
    "protobuf-rust": "protobuf",
}

_PATCHES = {
    "tempfile": ["@netsim//cargo/build:tempfile.patch"],
}

def _empty_repo_impl(repository_ctx):
    repository_ctx.file("BUILD.bazel", 'cc_library(name = "libslirp", visibility = ["//visibility:public"])\n')

_empty_repo = repository_rule(implementation = _empty_repo_impl)

def _lrc_impl(module_ctx):
    is_standalone = module_ctx.getenv("NETSIM_STANDALONE") == "1"

    if is_standalone:
        for repo_name, (version, sha256) in _CRATES.items():
            crate = _CRATE_NAMES.get(repo_name, repo_name)
            patches = _PATCHES.get(repo_name, [])
            http_archive(
                name = repo_name,
                build_file = "@netsim//cargo/build:{}.BUILD.bazel".format(repo_name),
                patch_args = ["-p1"] if patches else [],
                patches = patches,
                sha256 = sha256,
                strip_prefix = "{}-{}".format(crate, version),
                type = "tar.gz",
                urls = ["https://static.crates.io/crates/{}/{}-{}.crate".format(crate, crate, version)],
            )
        http_archive(
            name = "casimir",
            integrity = "sha256-pocNW03OHJWYs7F3wmXsNS2U8OceIjiu0Flns/2Ft3c=",
            strip_prefix = "casimir-50476e85f54349178c455844039b62ce9261e5f3",
            urls = ["https://github.com/google/casimir/archive/50476e85f54349178c455844039b62ce9261e5f3.tar.gz"],
        )
        git_repository(
            name = "rootcanal",
            remote = "https://android.googlesource.com/platform/tools/rootcanal",
            commit = "eb9752a1aa72d347a164bfb03ba223b0f6aee5ba",
        )
        _empty_repo(
            name = "libslirp",
        )
        return module_ctx.extension_metadata(root_module_direct_deps = "all", root_module_direct_dev_deps = [], reproducible = True)

    # Compute the repo root dynamically from the location of this module
    netsim_dir = module_ctx.path(Label("//:MODULE.bazel")).dirname
    workspace_root = str(netsim_dir.dirname.dirname)

    android_crates_io = "/".join([workspace_root, "external", "rust", "android-crates-io", "crates"])

    # 1. Custom Path Crates
    _CUSTOM_PATH_CRATES = [
        struct(name = "getrandom_v0_2", path = "/".join([workspace_root, "external", "rust", "android-crates-io-pinned", "crates", "getrandom"])),
        struct(name = "pica", path = workspace_root + "/third_party/rust/crates/pica"),
        struct(name = "protobuf-rust", path = android_crates_io + "/protobuf"),
    ]

    for crate in _CUSTOM_PATH_CRATES:
        new_local_repository(
            name = crate.name,
            build_file = "@netsim//cargo/build:{}.BUILD.bazel".format(crate.name),
            path = crate.path,
        )

    # 2. Patched Dependencies (tempfile)
    patched_new_local_repository(
        name = "tempfile",
        build_file = "@netsim//cargo/build:tempfile.BUILD.bazel",
        path = "{}/tempfile".format(android_crates_io),
        patches = _PATCHES["tempfile"],
    )

    # 3. Standard Android Crates
    _SPECIAL_CRATES = {crate.name: True for crate in _CUSTOM_PATH_CRATES} | {"tempfile": True}
    for crate_name in _CRATES.keys():
        if crate_name in _SPECIAL_CRATES:
            continue
        new_local_repository(
            name = crate_name,
            build_file = "@netsim//cargo/build:{}.BUILD.bazel".format(crate_name),
            path = "{}/{}".format(android_crates_io, crate_name),
        )

    local_repository(
        name = "casimir",
        path = "{}/packages/modules/Nfc/libnfc-nci/tools/casimir".format(workspace_root),
    )
    local_repository(
        name = "rootcanal",
        path = "{}/tools/rootcanal".format(workspace_root),
    )
    local_repository(
        name = "libslirp",
        path = "{}/third_party/libslirp".format(workspace_root),
    )

    return module_ctx.extension_metadata(root_module_direct_deps = "all", root_module_direct_dev_deps = [], reproducible = True)

lrc = module_extension(
    implementation = _lrc_impl,
    tag_classes = {},
    environ = ["NETSIM_STANDALONE"],
)
