// Minimal shim replacing the parts of cstimer's app that poly3dlib.js / threemin.js expect.
window.DEBUG = false;
window.$ = window.$ || {};
$.isArray = Array.isArray;
$.now = function() { return Date.now(); };

// Tiny jQuery-alike, only what threemin.js needs: $('<div>'), .css(), .append(), .attr(), [0]
function miniQ(sel) {
  var el;
  if (typeof sel === 'string' && sel[0] === '<') {
    var tag = sel.match(/<(\w+)/)[1];
    el = document.createElement(tag);
  } else if (sel instanceof Element) {
    el = sel;
  } else {
    el = document.createElement('div');
  }
  var wrapped = {
    0: el,
    css: function(k, v) { if (typeof k === 'object') { for (var p in k) el.style[p] = k[p]; } else { el.style[k] = v; } return wrapped; },
    attr: function(k, v) { if (v === undefined) return el.getAttribute(k); el.setAttribute(k, v); return wrapped; },
    append: function(child) { el.appendChild(child instanceof Element ? child : child[0]); return wrapped; },
    appendTo: function(parent) { (parent instanceof Element ? parent : parent[0]).appendChild(el); return wrapped; },
    width: function() { return el.clientWidth; },
    height: function() { return el.clientHeight; },
    find: function() { return miniQ(document.createElement('div')); },
    empty: function() { el.innerHTML = ''; return wrapped; }
  };
  return wrapped;
}
window.$fn = miniQ; // we call this instead of jQuery's $(...)