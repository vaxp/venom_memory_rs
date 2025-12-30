#include "venom_safe.h"
#include <string.h>

/**
 * --- OOP CLASS DEFINITION ---
 */
CLASS(Player) {
    char name[50];
    int health;
    
    // Methods
    METHOD(void, attack, int damage);
};

// Method implementations
void player_attack(void *self, int damage) {
    Player *p = (Player *)self;
    p->health -= damage;
    printf("[Battle] Player %s took %d damage! Health: %d\n", p->name, damage, p->health);
}

void player_destroy(void *self) {
    Player *p = (Player *)self;
    printf("[Cleanup] Releasing Player %s resources...\n", p->name);
}

// Constructor
Player* Player_new(const char *name, int health) {
    Player *p = malloc(sizeof(Player));
    strncpy(p->name, name, 49);
    p->health = health;
    p->attack = player_attack;
    p->destroy = player_destroy;
    return p;
}

/**
 * --- MAIN ---
 */
int main() {
    printf("--- VenomSafe OOP & Smart Pointer Demo ---\n");

    // 1. Manual OOP (The old way)
    Player *hero = NEW(Player, "AncientHero", 100);
    hero->attack(hero, 20);
    DELETE(hero); // Manual cleanup required

    printf("\n--- Starting RAII Session ---\n");
    {
        // 2. RAW Smart Pointer
        vptr int *secret_code = malloc(sizeof(int));
        *secret_code = 1337;
        printf("[RAII] Secret code: %d (will auto-free)\n", *secret_code);

        // 3. OBJECT Smart Pointer (Auto-destroy & Auto-free)
        printf("\n[RAII] Creating vobj Player (will auto-cleanup)...\n");
        vobj Player *bot = NEW(Player, "AutoCleaner", 50);
        bot->attack(bot, 10);
    }
    printf("--- RAII Session Ended ---\n");

    return 0;
}
