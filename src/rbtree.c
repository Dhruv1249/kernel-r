#include <stddef.h>
#include <stdint.h>

#define RB_BLACK 0
#define RB_RED 1

struct SchedNode {
  uint64_t vruntime;
  uint64_t task_id;
  uint64_t deadline;
  uint64_t min_deadline;
  struct SchedNode *left;
  struct SchedNode *right;
  uintptr_t
      parent_and_color; // Bit 0 is color, remaining bits are parent pointer
};

// Expose the function signatures
void rbtree_insert(struct SchedNode **root, struct SchedNode *new_node);
void rb_insert_fixup(struct SchedNode **root, struct SchedNode *node);
struct SchedNode *rbtree_leftmost(struct SchedNode *root);


static inline uint64_t min_u64(uint64_t a, uint64_t b) { return (a < b) ? a : b; }

// Recalculates the minimum deadline for a node based on its children
static inline void rb_update_min_deadline(struct SchedNode *n) {
    if (!n) return;
    n->min_deadline = n->deadline;
    if (n->left) n->min_deadline = min_u64(n->min_deadline, n->left->min_deadline);
    if (n->right) n->min_deadline = min_u64(n->min_deadline, n->right->min_deadline);
}

// Extract color
static inline int rb_color(const struct SchedNode *n) {
  return n->parent_and_color & 1;
}

// Extract parent
// Using ~3UL clears the bottom two bits just to be extra safe with alignment
// Reserving 2 bits for future use
static inline struct SchedNode *rb_parent(const struct SchedNode *n) {
  return (struct SchedNode *)(n->parent_and_color &
                              ~3UL); // UL is unsigned long
}

// Set color
static inline void rb_set_color(struct SchedNode *n, int color) {
  n->parent_and_color = (n->parent_and_color & ~1UL) | color;
}

// Set parent (Keep existing color, overwrite pointer)
static inline void rb_set_parent(struct SchedNode *n,
                                 struct SchedNode *parent) {
  n->parent_and_color = (n->parent_and_color & 1) | ((uintptr_t)parent & ~3UL);
}

void rb_rotate_left(struct SchedNode **root, struct SchedNode *x) {
  struct SchedNode *y = x->right;
  if (!y)
    return;

  //  Move y's left subtree to x's right
  x->right = y->left;
  if (y->left) {
    rb_set_parent(y->left, x);
  }

  // Link y to x's parent
  struct SchedNode *x_parent = rb_parent(x);
  if (!x_parent) {
    *root = y;
    rb_set_parent(y, NULL); // The new root has no parent
  } else {
    rb_set_parent(y, x_parent);
    if (x == x_parent->left) {
      x_parent->left = y;
    } else {
      x_parent->right = y;
    }
  }

  // Put x under y
  y->left = x;
  rb_set_parent(x, y);
}

void rb_rotate_right(struct SchedNode **root, struct SchedNode *y) {
  struct SchedNode *x = y->left;
  if (!x)
    return;

  // Move x's right subtree to y's left
  y->left = x->right;
  if (x->right) {
    rb_set_parent(x->right, y);
  }

  // Link x to y's parent
  struct SchedNode *y_parent = rb_parent(y);
  if (!y_parent) {
    *root = x;
    rb_set_parent(x, NULL); // The new root has no parent
  } else {
    rb_set_parent(x, y_parent);
    if (y == y_parent->left) {
      y_parent->left = x;
    } else {
      y_parent->right = x;
    }
  }

  x->right = y;
  rb_set_parent(y, x);
}

void rbtree_insert(struct SchedNode **root, struct SchedNode *node) {
  node->left = NULL;
  node->right = NULL;

  if (!*root) {
    *root = node;
    rb_set_parent(node, NULL);
    rb_set_color(node, RB_BLACK); // Root is always Black
    return;
  }

  struct SchedNode *parent = NULL;
  struct SchedNode *current = *root;

  while (current) {
    parent = current;
    if (node->vruntime < current->vruntime) {
      current = current->left;
    } else {
      current = current->right;
    }
  }

  rb_set_parent(node, parent);
  rb_set_color(node, RB_RED); // New nodes are always Red

  if (node->vruntime < parent->vruntime) {
    parent->left = node;
  } else {
    parent->right = node;
  }

  // Restore Red-Black invariants
  rb_insert_fixup(root, node);
}

void rb_insert_fixup(struct SchedNode **root, struct SchedNode *node) {
  // Loop while node is not root, and its parent is RED
  while (node != *root && rb_color(rb_parent(node)) == RB_RED) {
    struct SchedNode *parent = rb_parent(node);
    struct SchedNode *grandparent = rb_parent(parent);

    // LEFT SYMMETRY: Parent is the left child of Grandparent
    if (parent == grandparent->left) {
      struct SchedNode *uncle = grandparent->right; // Uncle is on the right

      // CASE 1: Uncle is RED
      if (uncle && rb_color(uncle) == RB_RED) {
        rb_set_color(parent, RB_BLACK);
        rb_set_color(uncle, RB_BLACK);
        rb_set_color(grandparent, RB_RED);
        node = grandparent; // Move our pointer up to check the grandparent
      } else {
        // CASE 2: The Triangle (Node is on the "inside")
        if (node == parent->right) {
          node = parent;              // Shift our focus to the parent
          rb_rotate_left(root, node); // Rotate left to straighten the line
          parent = rb_parent(node);   // Update parent pointer after rotation
        }

        // CASE 3: The Line (Node is on the "outside")
        // Now we are guaranteed to be a straight line.
        rb_set_color(parent, RB_BLACK);
        rb_set_color(grandparent, RB_RED);
        rb_rotate_right(root,
                        grandparent); // Push the heavy left side to the right
      }
    }
    // RIGHT SYMMETRY: Parent is the right child of Grandparent
    else {
      struct SchedNode *uncle = grandparent->left; // Uncle is on the left

      // CASE 1: Uncle is RED
      if (uncle && rb_color(uncle) == RB_RED) {
        rb_set_color(parent, RB_BLACK);
        rb_set_color(uncle, RB_BLACK);
        rb_set_color(grandparent, RB_RED);
        node = grandparent; // Move our pointer up to check the grandparent
      } else {
        // CASE 2: The Triangle (Node is on the "inside")
        if (node == parent->left) {
          node = parent;               // Shift our focus to the parent
          rb_rotate_right(root, node); // Rotate left to straighten the line
          parent = rb_parent(node);    // Update parent pointer after rotation
        }

        // CASE 3: The Line (Node is on the "outside")
        // Now we are guaranteed to be a straight line.
        rb_set_color(parent, RB_BLACK);
        rb_set_color(grandparent, RB_RED);
        rb_rotate_left(root,
                       grandparent); // Push the heavy left side to the right
      }
    }
  }

  // The Root Rule.
  // No matter what happened, ensure the absolute root is Black.
  rb_set_color(*root, RB_BLACK);
}

// Helper to safely get color even if node is NULL (NULL nodes are implicitly
// BLACK)
static inline int rb_color_safe(struct SchedNode *n) {
  return n ? rb_color(n) : RB_BLACK;
}

void rb_remove_fixup(struct SchedNode **root, struct SchedNode *x,
                     struct SchedNode *x_parent) {
  struct SchedNode *w; // 'w' will be x's sibling

  // Loop until we reach the root, or x becomes RED (which we can just paint
  // BLACK to fix)
  while (x != *root && rb_color_safe(x) == RB_BLACK) {

    // LEFT SYMMETRY: x is the left child
    if (x == x_parent->left) {
      w = x_parent->right;

      // CASE 1: Sibling 'w' is RED
      if (rb_color_safe(w) == RB_RED) {
        rb_set_color(x_parent, RB_RED);
        rb_set_color(w, RB_BLACK);
        rb_rotate_left(root, x_parent);
        w = x_parent->right;
      }

      // CASE 2: Sibling 'w' is BLACK, and BOTH of w's children are BLACK
      else if (rb_color_safe(w->left) == RB_BLACK &&
               rb_color_safe(w->right) == RB_BLACK) {
        rb_set_color(w, RB_RED);
        x = x_parent;
        x_parent = rb_parent(x);
      }

      // CASE 3: Sibling 'w' is BLACK, w's left child is RED, w's right child is
      // BLACK
      else if (rb_color_safe(w->left) == RB_RED &&
               rb_color_safe(w->right) == RB_BLACK) {
        rb_set_color(w->left, RB_BLACK);
        rb_set_color(w, RB_RED);
        rb_rotate_right(root, w);
        w = x_parent->right;
      }

      // CASE 4: Sibling 'w' is BLACK, and w's right child is RED
      else {
        rb_set_color(w, rb_color(x_parent));
        rb_set_color(x_parent, RB_BLACK);
        rb_set_color(w->right, RB_BLACK);
        rb_rotate_left(root, x_parent);
        x = *root;
      }
    }
    // RIGHT SYMMETRY: x is the right child (Exact mirror of above!)
    else {
      w = x_parent->left;

      if (rb_color_safe(w) == RB_RED) {
        rb_set_color(x_parent, RB_RED);
        rb_set_color(w, RB_BLACK);
        rb_rotate_right(root, x_parent);
        w = x_parent->left;
      }

      else if (rb_color_safe(w->left) == RB_BLACK &&
               rb_color_safe(w->right) == RB_BLACK) {
        rb_set_color(w, RB_RED);
        x = x_parent;
        x_parent = rb_parent(x);
      }

      else if (rb_color_safe(w->right) == RB_RED &&
               rb_color_safe(w->left) == RB_BLACK) {
        rb_set_color(w->right, RB_BLACK);
        rb_set_color(w, RB_RED);
        rb_rotate_left(root, w);
        w = x_parent->left;
      }

      else {
        rb_set_color(w, rb_color(x_parent));
        rb_set_color(x_parent, RB_BLACK);
        rb_set_color(w->left, RB_BLACK);
        rb_rotate_right(root, x_parent);
        x = *root;
      }
    }
  }

  // Finally, whatever x ended up as, paint it BLACK to absorb the extra
  // blackness.
  if (x)
    rb_set_color(x, RB_BLACK);
}

static inline void rb_transplant(struct SchedNode **root, struct SchedNode *u,
                                 struct SchedNode *v) {
  if (!rb_parent(u))
    *root = v;
  else if (u == rb_parent(u)->left)
    rb_parent(u)->left = v;
  else
    rb_parent(u)->right = v;
  if (v)
    rb_set_parent(v, rb_parent(u));
}

void rbtree_remove(struct SchedNode **root, struct SchedNode *z) {
  struct SchedNode *y = z, *x;
  struct SchedNode *x_parent = NULL;
  int y_original_color = rb_color(y);

  if (!z->left) {
    x = z->right;
    x_parent = rb_parent(z);
    rb_transplant(root, z, z->right);
  } else if (!z->right) {
    x = z->left;
    x_parent = rb_parent(z);
    rb_transplant(root, z, z->left);
  } else {
    y = rbtree_leftmost(z->right);
    y_original_color = rb_color(y);
    x = y->right;

    if (rb_parent(y) == z) {
      x_parent = y;

    } else {
      x_parent = rb_parent(y);
      rb_transplant(root, y, x);
      y->right = z->right;
      if (y->right)
        rb_set_parent(y->right, y);
    }

    rb_transplant(root, z, y);
    y->left = z->left;
    if (y->left)
      rb_set_parent(y->left, y);

    rb_set_color(y, rb_color(z));
  }

  if (y_original_color == RB_BLACK)
    rb_remove_fixup(root, x, x_parent);
}
struct SchedNode *rbtree_leftmost(struct SchedNode *root) {
  if (!root)
    return NULL;
  while (root->left) {
    root = root->left;
  }
  return root;
}

struct SchedNode* rbtree_pick_eevdf(struct SchedNode* root, uint64_t system_vruntime) {
    struct SchedNode* current = root;
    struct SchedNode* best = NULL;
    
    while (current) {
        if (current->vruntime <= system_vruntime) {
            // Current is eligible! Check if it has the best deadline so far.
            if (!best || current->deadline < best->deadline) {
                best = current;
            }
            
            // Left children are definitely eligible. Check if they have a better deadline.
            if (current->left && current->left->min_deadline < (best ? best->deadline : UINT64_MAX)) {
                current = current->left;
                continue;
            }
            // Right children might be eligible. Check if they have a better deadline.
            if (current->right && current->right->min_deadline < (best ? best->deadline : UINT64_MAX)) {
                current = current->right;
                continue;
            }
            break; // No better paths
        } else {
            // Current is NOT eligible. Right children are definitely not eligible either.
            // We MUST go left to find smaller vruntimes.
            current = current->left;
        }
    }
    
    // Fallback: If no tasks are strictly eligible, just pick the leftmost node.
    return best ? best : rbtree_leftmost(root);
}
