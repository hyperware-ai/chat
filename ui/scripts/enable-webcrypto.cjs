// Ensures Vite (which expects WebCrypto) works under older Node versions.
const { webcrypto } = require('crypto');
if (webcrypto && !global.crypto) {
  global.crypto = webcrypto;
}
