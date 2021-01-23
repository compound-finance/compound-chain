const { log, error } = require('../util/log');

function genPort() {
  // TODO: Actually check port is free?
  return Math.floor(Math.random() * (65535 - 1024)) + 1024;
}

async function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

async function until(cond, opts = {}) {
  let options = {
    delay: 5000,
    retries: null,
    message: null,
    ...opts
  };

  let start = +new Date();

  if (await cond()) {
    return;
  } else {
    if (options.message) {
      log(options.message);
    }
    await sleep(options.delay + start - new Date());
    return await until(cond, {
      ...options,
      retries: options.retries === null ? null : options.retries - 1
    });
  }
}

function merge(x, y) {
  Object.entries(y).forEach(([key, val]) => {
    if (typeof (x[key]) === 'object' && typeof (val) === 'object' && !Array.isArray(x[key])) {
      x[key] = merge(x[key], val);
    } else {
      x[key] = val;
    }
  });

  return x;
}

function getInfoKey(info, key, type) {
  if (!info.hasOwnProperty(key)) {
    throw new Error(`Expected key \`${key}\` in ${JSON.stringify(info)} for ${type}`);
  }

  return info[key];
}

function stripHexPrefix(str) {
  if (str.startsWith('0x')) {
    return str.slice(2);
  } else {
    return str;
  }
}

function lookupBy(cls, lookupKey, arr, lookup) {
  if (lookup instanceof cls) {
    return lookup;
  } else if (typeof (lookup) === 'string') {
    let el = arr.find((el) => el[lookupKey] === lookup);

    if (!el) {
      throw new Error(`Unknown ${cls.name} for scenario: ${lookup} [${cls.name}s: ${arr.map((el) => el[lookupKey]).join(', ')}]`);
    } else {
      return el;
    }
  } else {
    throw new Error(`Don't know how to lookup ${cls.name} from \`${JSON.stringify(lookup)}\``);
  }
}

module.exports = {
  genPort,
  sleep,
  until,
  merge,
  getInfoKey,
  stripHexPrefix,
  lookupBy,
};
