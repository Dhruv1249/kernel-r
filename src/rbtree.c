#include <stddef.h>
#include <stdint.h>

#define RB_BLACK 0
#define RB_RED 1

struct SchedNode {
  uint64_t vruntime;
  uint64_t task_id;
  struct SchedNode *left;
  struct SchedNode *right;
  uintptr_t
      parent_and_color; // Bit 0 is color, remaining bits are parent pointer
};

// Expose the function signatures
void rbtree_insert(struct SchedNode **root, struct SchedNode *new_node);

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
  // rb_insert_fixup(root, node);
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

struct SchedNode *rbtree_leftmost(struct SchedNode *root) {
  if (!root)
    return NULL;
  while (root->left) {
    root = root->left;
  }
  return root;
}
