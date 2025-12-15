// Monkey-patch Node's crypto module so Vite can call crypto.getRandomValues.
const crypto = require('crypto');
if (typeof crypto.getRandomValues !== 'function' && crypto.webcrypto?.getRandomValues) {
  crypto.getRandomValues = crypto.webcrypto.getRandomValues.bind(crypto.webcrypto);
}
if (crypto.webcrypto && !global.crypto) {
  global.crypto = crypto.webcrypto;
}
